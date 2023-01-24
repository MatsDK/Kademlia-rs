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
    pending_event: Option<(Key, Kad)>,
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

    pub fn local_key(&self) -> &Key {
        &self.routing_table.local_key
    }

    pub fn add_address(&mut self, key: &Key, addr: Multiaddr) {
        self.routing_table.insert(key, addr);
    }

    pub fn find_node(&mut self, target: &Key) {
        let peers = self.routing_table.closest_nodes(target);
        self.queries.add_query(
            peers,
            KademliaEvent::FindNode {
                target: target.clone(),
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

    fn handle_pool_event(&mut self, ev: PoolEvent) {
        match ev {
            PoolEvent::NewConnection { key, remote_addr } => {
                println!("Established connection {key} {remote_addr}");
                let endpoint = socketaddr_to_multiaddr(&remote_addr);
                self.add_address(&key, endpoint);
                self.connected_peers.insert(key);
            }
            PoolEvent::Request { key, event } => {
                println!("new request from {key}: {event:?}")
            }
        }
    }

    fn handle_query_pool_event(&mut self, ev: NodeEvent) -> Option<()> {
        match ev {
            NodeEvent::Dial { peer_id } => {}
            NodeEvent::Notify { peer_id, event } => {
                self.pending_event = Some((peer_id, event));
            }
            NodeEvent::GenerateEvent => return Some(()),
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
                        return Poll::Ready(NodeEvent::GenerateEvent);
                    }
                    QueryPoolState::Waiting(Some((q, key))) => {
                        if self.connected_peers.contains(&key) {
                            self.queued_events.push_back(NodeEvent::Notify {
                                peer_id: key,
                                event: Kad::Request { query_id: q.id(), event: q.get_event() },
                            });
                        } else {
                            println!("should connect to peer {key}");
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

    fn poll_next_event(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        loop {
            // If there is a pending query, let it make progress
            // otherwise, let the query_pool make progress
            match self.pending_event.take() {
                Some((key, ev)) => match self.pool.get_connection(&key) {
                    Some(conn) => match conn.send_event(ev, cx) {
                        None => continue,
                        Some(ev) => {
                            self.pending_event = Some((key, ev));
                        }
                    },
                    None => continue,
                },
                None => {
                    // Poll next query
                    match self.poll_next_query(cx) {
                        Poll::Pending => {}
                        Poll::Ready(ev) => {
                            println!("{ev:?}");
                            if let Some(ev) = self.handle_query_pool_event(ev) {
                                return Poll::Ready(());
                            }

                            continue;
                        }
                    }
                }
            }

            // Poll the connection pool for updates
            match self.pool.poll(cx) {
                Poll::Pending => {}
                Poll::Ready(connection_ev) => {
                    self.handle_pool_event(connection_ev);
                    return Poll::Ready(());
                }
            }

            // Poll the listener for new connections
            match self.transport.poll(cx) {
                Poll::Pending => {}
                Poll::Ready(transport_ev) => {
                    self.handle_transport_event(transport_ev);
                    return Poll::Ready(());
                }
            }

            return Poll::Pending;
        }
    }
}

impl Stream for KademliaNode {
    type Item = ();

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
pub enum Kad {
    Request {
        query_id: usize,
        event: KademliaEvent
    },
    Response {
        query_id: usize,
        event: KademliaEvent
    },
    Ping(Key)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum KademliaEvent {
    FindNode { target: Key },
}

#[derive(Debug, Clone)]
pub enum NodeEvent {
    Dial { peer_id: Key },
    Notify { peer_id: Key, event: Kad },
    GenerateEvent,
}
