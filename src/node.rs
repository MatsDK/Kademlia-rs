use futures::{stream::FusedStream, Stream};
use multiaddr::Multiaddr;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    io,
    num::NonZeroUsize,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    key::Key,
    pool::{Pool, PoolEvent},
    query::{PutRecordStep, Query, QueryInfo, QueryPool, QueryPoolState, Quorum},
    routing::{Node, NodeStatus, RoutingTable},
    store::{Record, RecordStore},
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
    store: RecordStore,
}

impl KademliaNode {
    pub async fn new(key: Key, addr: impl Into<Multiaddr>) -> io::Result<Self> {
        let addr = addr.into();
        println!(">> Listening {addr} >> {key}");
        let transport = Transport::new(&addr).await.unwrap();

        Ok(Self {
            routing_table: RoutingTable::new(key.clone()),
            transport,
            pool: Pool::new(key.clone()),
            queries: QueryPool::new(),
            connected_peers: HashSet::new(),
            queued_events: VecDeque::new(),
            pending_event: None,
            store: RecordStore::new(key),
        })
    }

    pub fn dial(&mut self, addr: impl Into<Multiaddr>) -> Result<(), ()> {
        let addr = addr.into();
        let dial = match self.transport.dial(&addr) {
            Err(e) => {
                eprintln!("{:?}", e);
                return Err(());
            }
            Ok(dial) => dial,
        };

        self.pool.add_outgoing(dial, addr);

        Ok(())
    }

    pub fn dial_with_peer_id(&mut self, peer: Key) -> Result<(), ()> {
        let addr = self.find_addr_for_peer(&peer);
        if let Some(addr) = addr {
            self.dial(addr.clone())
        } else {
            eprintln!("There is no addr in routing table for {peer}");
            Err(())
        }
    }

    pub fn find_addr_for_peer(&self, peer: &Key) -> Option<&Multiaddr> {
        // Check if there is an address for the peer returned in an ongoing query.
        for query in self.queries.iter() {
            if let Some(addr) = query.get_addr_for_peer(peer) {
                return Some(addr);
            }
        }

        // Otherwise, check if the address is already in the routing table.
        self.routing_table.get_addr(peer)
    }

    pub fn bootstrap(&mut self) -> io::Result<()> {
        let local_key = self.routing_table.local_key.clone();

        let peers = self.routing_table.closest_nodes(&local_key);
        let query_info = QueryInfo::Bootstrap {
            target: local_key.clone(),
        };
        self.queries.add_query(local_key.clone(), peers, query_info);

        Ok(())
    }

    pub fn put_record(&mut self, record: Record, quorum: Quorum) -> Result<(), String> {
        self.store.put(record.clone()).unwrap();

        let peers = self.routing_table.closest_nodes(&record.key);
        let target = record.key.clone();

        let query_info = QueryInfo::PutRecord {
            record,
            step: PutRecordStep::FindNodes,
            quorum: quorum.into(),
        };
        self.queries.add_query(target, peers, query_info);

        Ok(())
    }

    pub fn get_record(&mut self, key: &Key) -> Option<&Record> {
        if let Some(record) = self.store.get(key) {
            return Some(record);
        }

        let peers = self.routing_table.closest_nodes(key);
        let query_info = QueryInfo::GetRecord {
            key: key.clone(),
            found_a_record: false,
        };

        self.queries.add_query(*key, peers, query_info);

        None
    }

    // Removes the record locally, only remove if you are the publisher
    pub fn remove_record(&mut self, key: &Key) {
        if let Some(record) = self.store.get(key) {
            if record.publisher.as_ref() == Some(self.local_key()) {
                self.store.remove(key);
            }
        }
    }

    pub fn local_key(&self) -> &Key {
        &self.routing_table.local_key
    }

    pub fn add_address(&mut self, key: &Key, addr: Multiaddr) {
        let status = if self.connected_peers.contains(key) {
            NodeStatus::Connected
        } else {
            NodeStatus::Disconnected
        };

        self.routing_table
            .update_node_status(key.clone(), Some(addr), status);
    }

    // fn update_node_status(&mut self, key: Key, addr: Option<Multiaddr>, status: NodeStatus) {
    //     self.routing_table.update_node_status(key, addr, status);
    // }

    pub fn find_node(&mut self, target: &Key) {
        let peers = self.routing_table.closest_nodes(target);

        let query_info = QueryInfo::FindNode {
            target: target.clone(),
        };
        self.queries.add_query(target.clone(), peers, query_info);
    }

    fn record_received(&mut self, peer_id: Key, record: Record, request_id: usize) {
        println!("stored record successfully: {record}");
        self.store.put(record).unwrap();
        self.queued_events.push_back(NodeEvent::Notify {
            peer_id,
            event: KademliaEvent::PutRecordRes { request_id },
        });
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

    fn handle_incoming_event(&mut self, peer_id: Key, ev: KademliaEvent) {
        match ev {
            KademliaEvent::FindNodeReq { target, request_id } => {
                let closest_nodes = self.routing_table.closest_nodes(&target);
                self.queued_events.push_back(NodeEvent::Notify {
                    peer_id,
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
                let local_key = self.local_key().clone();

                if let Some(query) = self.queries.get_mut(&request_id) {
                    query.on_success(&peer_id, closest_nodes, local_key);
                }
            }
            KademliaEvent::PutRecordReq { record, request_id } => {
                self.record_received(peer_id, record, request_id);
            }
            KademliaEvent::PutRecordRes { request_id } => {
                let local_key = self.local_key().clone();

                if let Some(query) = self.queries.get_mut(&request_id) {
                    query.on_success(&peer_id, vec![], local_key);

                    if let QueryInfo::PutRecord {
                        step: PutRecordStep::PutRecord { put_success },
                        quorum,
                        ..
                    } = &mut query.query_info
                    {
                        put_success.push(peer_id);

                        // Quorum is reached -> query should be finished
                        if put_success.len() >= quorum.get() {
                            query.finish()
                        }
                    }
                }
            }
            KademliaEvent::GetRecordReq { key, request_id } => {
                let record = self.store.get(&key);
                self.queued_events.push_back(NodeEvent::Notify {
                    peer_id,
                    event: KademliaEvent::GetRecordRes {
                        record: record.cloned(),
                        request_id,
                    },
                })
            }
            KademliaEvent::GetRecordRes { record, request_id } => {
                let local_key = self.local_key().clone();

                if let Some(query) = self.queries.get_mut(&request_id) {
                    if let QueryInfo::GetRecord {
                        ref mut found_a_record,
                        ..
                    } = query.query_info
                    {
                        if let Some(record) = record {
                            *found_a_record = true;
                            let out_ev = OutEvent::OutBoundQueryProgressed {
                                result: QueryResult::GetRecord(GetRecordResult::FoundRecord(
                                    record,
                                )),
                            };
                            self.queued_events
                                .push_back(NodeEvent::GenerateEvent(out_ev));
                        }
                    }
                    // println!(
                    //     "Query: {query:?}, record received from {peer_id} {record}",
                    //     record = record.unwrap()
                    // );

                    query.on_success(&peer_id, vec![], local_key);
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

                // Once the connection is established send all the queued events to that peer.
                for event in self
                    .queries
                    .iter_mut()
                    .filter_map(|q| q.get_waiting_events(&key))
                    .flatten()
                {
                    self.queued_events.push_back(NodeEvent::Notify {
                        peer_id: key.clone(),
                        event,
                    });
                }

                let out_ev = OutEvent::ConnectionEstablished(key);
                return Some(NodeEvent::GenerateEvent(out_ev));
            }
            PoolEvent::Request { key, event } => {
                self.handle_incoming_event(key, event);
            }
            PoolEvent::ConnectionClosed { key, error } => {
                // Set the state for the closed peer to `PeerState::Failed`
                for query in self.queries.iter_mut() {
                    query.on_failure(&key);
                }
                self.connected_peers.remove(&key);

                self.routing_table
                    .update_node_status(key, None, NodeStatus::Disconnected);

                eprintln!("Connection closed with message: {}", error);
            }
        }
        None
    }

    fn handle_query_pool_event(&mut self, ev: NodeEvent) -> Option<NodeEvent> {
        match ev {
            NodeEvent::Dial { peer_id } => {
                if let Ok(()) = self.dial_with_peer_id(peer_id) {
                    // return NodeEvent::Dialing
                }
            }
            NodeEvent::Notify { peer_id, event } => {
                assert!(self.pending_event.is_none());
                self.pending_event = Some((peer_id, event));
            }
            ev => return Some(ev),
        }

        None
    }

    fn query_finished(&mut self, query: Query) -> Option<NodeEvent> {
        let query_result = query.result();
        match query_result.info {
            QueryInfo::FindNode { target } => {
                let out_ev = OutEvent::OutBoundQueryProgressed {
                    result: QueryResult::FindNode {
                        target,
                        nodes: query_result.keys(),
                    },
                };

                Some(NodeEvent::GenerateEvent(out_ev))
            }
            QueryInfo::PutRecord {
                record,
                step: PutRecordStep::FindNodes,
                quorum,
            } => {
                let target = record.key.clone();

                let query_info = QueryInfo::PutRecord {
                    record,
                    quorum,
                    step: PutRecordStep::PutRecord {
                        put_success: vec![],
                    },
                };

                // TODO: make sure this is a fixed set of peers iterator for `query_result.nodes`
                self.queries.add_query_with_id(
                    query_result.query_id,
                    target,
                    query_result.nodes,
                    query_info,
                );

                None
            }
            QueryInfo::PutRecord {
                record,
                step: PutRecordStep::PutRecord { put_success },
                quorum,
            } => {
                let key = record.key;
                let res = {
                    if put_success.len() >= quorum.get() {
                        Ok(PutRecordOk { key })
                    } else {
                        Err(PutRecordError::QuorumFailed {
                            key,
                            quorum,
                            successfull_peers: put_success,
                        })
                    }
                };

                let out_ev = OutEvent::OutBoundQueryProgressed {
                    result: QueryResult::PutRecord(res),
                };

                Some(NodeEvent::GenerateEvent(out_ev))
            }
            QueryInfo::GetRecord {
                key,
                found_a_record,
            } => {
                if found_a_record {
                    None
                } else {
                    let out_ev = OutEvent::OutBoundQueryProgressed {
                        result: QueryResult::GetRecord(GetRecordResult::NotFound(key)),
                    };
                    Some(NodeEvent::GenerateEvent(out_ev))
                }
                // let out_ev = OutEvent::OutBoundQueryProgressed {
                //     result: QueryResult::GetRecord { key },
                // };
                // Some(NodeEvent::GenerateEvent(out_ev))
            }
            QueryInfo::Bootstrap { .. } => {
                // TODO: should refresh some buckets
                let out_ev = OutEvent::OutBoundQueryProgressed {
                    result: QueryResult::Bootstrap,
                };
                Some(NodeEvent::GenerateEvent(out_ev))
            }
        }
    }

    fn poll_next_query(&mut self) -> Poll<NodeEvent> {
        loop {
            // first get all the queued events from previous iterations
            if let Some(ev) = self.queued_events.pop_front() {
                return Poll::Ready(ev);
            }

            loop {
                // poll the queries handler
                match self.queries.poll() {
                    QueryPoolState::Finished(q) => {
                        if let Some(e) = self.query_finished(q) {
                            return Poll::Ready(e);
                        };
                    }
                    QueryPoolState::Waiting(Some((q, key))) => {
                        if self.connected_peers.contains(&key) {
                            self.queued_events.push_back(NodeEvent::Notify {
                                peer_id: key,
                                event: q.get_request(),
                            });
                        } else {
                            let ev = q.get_request();
                            q.waiting_events.entry(key.clone()).or_default().push(ev);
                            self.queued_events
                                .push_back(NodeEvent::Dial { peer_id: key })
                        }
                    }
                    QueryPoolState::TimeOut(q) => {
                        println!(">> Query timed out: {q:?}");
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
                    match self.poll_next_query() {
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
        closest_nodes: Vec<Node>,
        request_id: usize,
    },
    PutRecordReq {
        record: Record,
        request_id: usize,
    },
    PutRecordRes {
        request_id: usize,
    },
    GetRecordReq {
        key: Key,
        request_id: usize,
    },
    GetRecordRes {
        record: Option<Record>,
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
    FindNode { nodes: Vec<Key>, target: Key },
    PutRecord(PutRecordResult),
    GetRecord(GetRecordResult),
    Bootstrap,
}

type PutRecordResult = Result<PutRecordOk, PutRecordError>;

#[derive(Debug, Clone)]
pub struct PutRecordOk {
    pub key: Key,
}

#[derive(Debug, Clone)]
pub enum PutRecordError {
    QuorumFailed {
        key: Key,
        quorum: NonZeroUsize,
        successfull_peers: Vec<Key>,
    },
}

#[derive(Debug, Clone)]
pub enum GetRecordResult {
    FoundRecord(Record),
    NotFound(Key),
}
