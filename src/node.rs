use futures::{stream::FusedStream, Stream};
use multiaddr::Multiaddr;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    io,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    key::Key,
    pool::{Pool, PoolEvent},
    query::{QueryPool, QueryPoolState},
    routing::RoutingTable,
    transport::{socketaddr_to_multiaddr, Transport, TransportEvent},
};

#[derive(Debug)]
pub struct KademliaNode {
    routing_table: RoutingTable,
    transport: Transport,
    pool: Pool,
    queries: QueryPool,
    connected_peers: HashSet<Key>,
    queued_events: VecDeque<NodeEvent>,
    pending_event: Option<(Key, KademliaEvent)>,
}

impl KademliaNode {
    pub async fn new(key: Key, addr: impl Into<Multiaddr>) -> io::Result<Self> {
        let addr = addr.into();
        println!(">> Listening {addr} >> {key}");
        let transport = Transport::new(&addr).await.unwrap();

        Ok(Self {
            routing_table: RoutingTable::new(key.clone()),
            transport,
            pool: Pool::new(key),
            queries: QueryPool::new(),
            connected_peers: HashSet::new(),
            queued_events: VecDeque::new(),
            pending_event: None,
        })
    }

    pub async fn dial(&mut self, addr: impl Into<Multiaddr>) -> io::Result<()> {
        let addr = addr.into();
        let stream = self.transport.dial(&addr).await.unwrap();

        self.pool.add_outgoing(stream, addr).await;

        Ok(())
    }

    pub async fn boostrap(&mut self) -> io::Result<()> {
        let local_key = self.routing_table.local_key.clone();

        let peers = self.routing_table.closest_nodes(&local_key);
        let query_id = self.queries.next_query_id();
        self.queries.add_query(
            local_key.clone(),
            peers,
            query_id,
            KademliaEvent::FindNodeReq {
                target: local_key,
                request_id: query_id,
            },
        );

        Ok(())
    }

    pub fn local_key(&self) -> &Key {
        &self.routing_table.local_key
    }

    pub fn add_address(&mut self, key: &Key, addr: Multiaddr) {
        self.routing_table.insert(key, addr);
    }

    pub fn find_node(&mut self, target: &Key) {
        let peers = self.routing_table.closest_nodes(target);
        let query_id = self.queries.next_query_id();

        self.queries.add_query(
            target.clone(),
            peers,
            query_id,
            KademliaEvent::FindNodeReq {
                target: target.clone(),
                request_id: query_id,
            },
        );
    }

    fn handle_transport_event(&mut self, ev: TransportEvent) {
        match ev {
            TransportEvent::Incoming {
                stream,
                socket_addr,
            } => {
                self.pool.add_incoming(stream, socket_addr);
            }
            TransportEvent::Error(e) => {
                println!("Got error {e}");
            }
        }
    }

    fn handle_incoming_event(&mut self, key: Key, ev: KademliaEvent) {
        match ev {
            KademliaEvent::FindNodeReq { target, request_id } => {
                let closest_nodes = self.routing_table.closest_nodes(&target);
                self.queued_events.push_back(NodeEvent::Notify {
                    peer_id: key,
                    event: KademliaEvent::FindNodeRes {
                        closest_nodes,
                        request_id,
                    },
                });
            }
            KademliaEvent::FindNodeRes {
                closest_nodes,
                request_id,
            } => {
                println!("response");
                let local_key = self.local_key().clone();

                if let Some(query) = self.queries.get_mut(&request_id) {
                    query.on_success(&key, closest_nodes, local_key);
                }
            }
            KademliaEvent::Ping { .. } => {}
        }
    }

    fn handle_pool_event(&mut self, ev: PoolEvent) -> Option<NodeEvent> {
        match ev {
            PoolEvent::NewConnection { key, remote_addr } => {
                let endpoint = socketaddr_to_multiaddr(&remote_addr);
                self.add_address(&key, endpoint);
                self.connected_peers.insert(key.clone());

                let out_ev = OutEvent::ConnectionEstablished(key);
                return Some(NodeEvent::GenerateEvent(out_ev));
            }
            PoolEvent::Request { key, event } => {
                self.handle_incoming_event(key, event);
            }
        }
        None
    }

    fn handle_query_pool_event(&mut self, ev: NodeEvent) -> Option<NodeEvent> {
        match ev {
            NodeEvent::Dial { peer_id } => {
                println!("dial {}", peer_id);
                // TODO: dial the peer
            }
            NodeEvent::Notify { peer_id, event } => {
                assert!(self.pending_event.is_none());
                self.pending_event = Some((peer_id, event));
            }
            ev => return Some(ev),
        }

        None
    }

    fn poll_next_query(&mut self, cx: &mut Context<'_>) -> Poll<NodeEvent> {
        loop {
            // first get all the queued events from previous iterations
            if let Some(ev) = self.queued_events.pop_front() {
                return Poll::Ready(ev);
            }

            loop {
                // poll the queries handler
                match self.queries.poll() {
                    QueryPoolState::Finished(q) => {
                        // TODO: match different query results
                        let out_ev = OutEvent::OutBoundQueryProgressed {
                            result: QueryResult::FindNode { nodes: vec![] },
                        };
                        return Poll::Ready(NodeEvent::GenerateEvent(out_ev));
                    }
                    QueryPoolState::Waiting(Some((q, key))) => {
                        if self.connected_peers.contains(&key) {
                            self.queued_events.push_back(NodeEvent::Notify {
                                peer_id: key,
                                event: q.get_event(),
                            });
                        } else {
                            self.queued_events
                                .push_back(NodeEvent::Dial { peer_id: key })
                        }
                    }
                    QueryPoolState::Idle | QueryPoolState::Waiting(None) => break,
                }
            }

            if self.queued_events.is_empty() {
                return Poll::Pending;
            }
        }
    }

    fn poll_next_event(&mut self, cx: &mut Context<'_>) -> Poll<OutEvent> {
        loop {
            // If there is a pending query, let it make progress
            // otherwise, let the `QueryPool` make progress.
            match self.pending_event.take() {
                Some((key, ev)) => {
                    self.pending_event = None;
                    match self.pool.get_connection(&key) {
                        Some(conn) => match conn.send_event(ev, cx) {
                            None => {
                                // successfully send request to peer
                                continue;
                            }
                            Some(ev) => {
                                self.pending_event = Some((key, ev));
                            }
                        },
                        None => continue,
                    }
                }
                None => {
                    // Poll next query
                    match self.poll_next_query(cx) {
                        Poll::Pending => {}
                        Poll::Ready(ev) => {
                            if let Some(NodeEvent::GenerateEvent(ev)) =
                                self.handle_query_pool_event(ev)
                            {
                                return Poll::Ready(ev);
                            }

                            continue;
                        }
                    }
                }
            }

            // Poll the connection pool for updates.
            match self.pool.poll(cx) {
                Poll::Pending => {}
                Poll::Ready(connection_ev) => {
                    if let Some(NodeEvent::GenerateEvent(ev)) =
                        self.handle_pool_event(connection_ev)
                    {
                        return Poll::Ready(ev);
                    }

                    continue;
                }
            }

            // Poll the listener for new connections.
            match self.transport.poll(cx) {
                Poll::Pending => {}
                Poll::Ready(transport_ev) => {
                    self.handle_transport_event(transport_ev);
                    return Poll::Ready(OutEvent::Other);
                }
            }

            return Poll::Pending;
        }
    }
}

impl Stream for KademliaNode {
    type Item = OutEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.poll_next_event(cx).map(Some)
    }
}

impl FusedStream for KademliaNode {
    fn is_terminated(&self) -> bool {
        false
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum KademliaEvent {
    FindNodeReq {
        target: Key,
        request_id: usize,
    },
    FindNodeRes {
        closest_nodes: Vec<Key>,
        request_id: usize,
    },
    Ping {
        target: Key,
    },
}

#[derive(Debug, Clone)]
pub enum NodeEvent {
    Dial { peer_id: Key },
    Notify { peer_id: Key, event: KademliaEvent },
    GenerateEvent(OutEvent),
}

#[derive(Debug, Clone)]
pub enum OutEvent {
    OutBoundQueryProgressed { result: QueryResult },
    ConnectionEstablished(Key),
    Other,
}

#[derive(Debug, Clone)]
pub enum QueryResult {
    FindNode { nodes: Vec<Key> },
}
