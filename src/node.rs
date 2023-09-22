use futures::{stream::FusedStream, Stream};
use multiaddr::Multiaddr;
use serde::{Deserialize, Serialize};
#[allow(unused)]
use std::{
    collections::{HashMap, HashSet, VecDeque},
    io,
    num::NonZeroUsize,
    pin::Pin,
    task::{Context, Poll},
};
use std::{
    fmt,
    time::{Duration, Instant},
};

use crate::{
    key::Key,
    pool::{Pool, PoolEvent},
    query::{
        PutRecordContext, PutRecordStep, Query, QueryId, QueryInfo, QueryPool, QueryPoolState,
        Quorum,
    },
    republish::RepublishJob,
    routing::{Node, NodeStatus, RoutingTable, UpdateResult},
    store::{Record, RecordStore},
    transport::{socketaddr_to_multiaddr, Transport, TransportEvent},
    JOB_MAX_NEW_QUERIES, K_VALUE, MAX_QUERIES,
};

#[derive(Debug, Clone)]
pub struct KademliaConfig {
    /// `None` indicates not expiration of records
    record_ttl: Option<Duration>,
    replication_interval: Option<Duration>,
    republish_interval: Option<Duration>,
}

impl Default for KademliaConfig {
    fn default() -> Self {
        Self {
            record_ttl: Some(Duration::from_secs(60 * 60 * 36)),
            replication_interval: Some(Duration::from_secs(60 * 60 * 1)),
            republish_interval: Some(Duration::from_secs(60 * 60 * 24)),
        }
    }
}

impl KademliaConfig {
    /// Set the TTL for records. It should be longer than the republication interval.
    /// `None` indicates records never expire. Default: 36h
    pub fn set_record_ttl(&mut self, record_ttl: Option<Duration>) -> &mut Self {
        self.record_ttl = record_ttl;
        self
    }

    /// Set the rereplication interval. This is used to ensure that records are
    /// always replicated to the nodes closest to the record. This operation does
    /// not extend the expiry of the record. This interval should be shorten than
    /// the publication interval.
    ///
    /// `None` means that stored records are never re-replicated. Default: 1h
    pub fn set_replication_interval(&mut self, interval: Option<Duration>) -> &mut Self {
        self.replication_interval = interval;
        self
    }

    /// Set the republish interval. This is used for extending the local lifetime of
    /// local records. This should be shorter than the default record TTL to ensure
    /// records do not expire before republishing them.
    ///
    /// `None` means that records are never republished. Default: 24h
    pub fn set_publication_interval(&mut self, interval: Option<Duration>) -> &mut Self {
        self.republish_interval = interval;
        self
    }
}

#[derive(Debug)]
pub struct KademliaNode {
    config: KademliaConfig,
    routing_table: RoutingTable,
    transport: Transport,
    pool: Pool,
    queries: QueryPool,
    connected_peers: HashSet<Key>,
    queued_events: VecDeque<NodeEvent>,
    pending_event: Option<(Key, KademliaEvent)>,
    store: RecordStore,
    addr: Multiaddr,
    republish_job: Option<RepublishJob>,
}

impl KademliaNode {
    pub async fn new(
        key: Key,
        addr: impl Into<Multiaddr>,
        config: KademliaConfig,
    ) -> io::Result<Self> {
        let addr = addr.into();
        let transport = Transport::new(&addr).await.unwrap();

        let republish_job = config.replication_interval.map(|replication_interval| {
            RepublishJob::new(
                key,
                config.record_ttl,
                replication_interval,
                config.republish_interval,
            )
        });

        Ok(Self {
            config,
            routing_table: RoutingTable::new(key.clone()),
            transport,
            pool: Pool::new(key.clone()),
            queries: QueryPool::new(),
            connected_peers: HashSet::new(),
            queued_events: VecDeque::new(),
            pending_event: None,
            store: RecordStore::new(key),
            addr,
            republish_job,
        })
    }

    pub fn get_addr(&self) -> &Multiaddr {
        &self.addr
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

    pub fn disconnect(&mut self, peer_id: Key) -> Result<(), ()> {
        let was_connected = self.connected_peers.contains(&peer_id);
        self.pool.disconnect(peer_id);

        if was_connected {
            Ok(())
        } else {
            Err(())
        }
    }

    pub fn bootstrap(&mut self) -> Result<QueryId, NoKnownPeers> {
        let local_key = self.routing_table.local_key.clone();

        let peers = self.routing_table.closest_nodes(&local_key);
        if peers.is_empty() {
            Err(NoKnownPeers)
        } else {
            let query_info = QueryInfo::Bootstrap {
                target: local_key.clone(),
            };
            Ok(self.queries.add_query(local_key.clone(), peers, query_info))
        }
    }

    pub fn put_record(&mut self, mut record: Record, quorum: Quorum) -> Result<QueryId, ()> {
        record.set_publisher(*self.local_key());
        // TODO: errors for store with `thiserror`
        self.store.put(record.clone()).unwrap();

        record.expires = record
            .expires
            .or_else(|| self.config.record_ttl.map(|ttl| Instant::now() + ttl));

        // TODO: make debug feature only
        self.queued_events
            .push_back(NodeEvent::GenerateEvent(OutEvent::StoreChanged(
                StoreChangedEvent::PutRecord {
                    record: record.clone(),
                },
            )));

        let peers = self.routing_table.closest_nodes(&record.key);
        let target = record.key.clone();

        let query_info = QueryInfo::PutRecord {
            record,
            step: PutRecordStep::FindNodes,
            context: PutRecordContext::Publish,
            quorum: quorum.into(),
        };
        Ok(self.queries.add_query(target, peers, query_info))
    }

    fn publish_record(
        &mut self,
        record: Record,
        quorum: Quorum,
        context: PutRecordContext,
    ) -> Result<QueryId, ()> {
        let target = record.key;
        let peers = self.routing_table.closest_nodes(&target);

        let query_info = QueryInfo::PutRecord {
            record,
            step: PutRecordStep::FindNodes,
            context,
            quorum: quorum.into(),
        };
        Ok(self.queries.add_query(target, peers, query_info))
    }

    pub fn get_record(&mut self, key: Key) -> QueryId {
        let record: Option<FoundRecord> = if let Some(record) = self.store.get(&key) {
            if record.is_expired(Instant::now()) {
                self.store.remove(&key);
                None
            } else {
                Some(FoundRecord {
                    source: self.local_key().clone(),
                    record: record.into_owned(),
                })
            }
        } else {
            None
        };

        let query_info = if record.is_some() {
            QueryInfo::GetRecord {
                key,
                found_a_record: true,
            }
        } else {
            QueryInfo::GetRecord {
                key,
                found_a_record: false,
            }
        };

        let peers = self.routing_table.closest_nodes(&key);
        let id = self.queries.add_query(key, peers, query_info);

        if let Some(record) = record {
            let out_ev = OutEvent::OutBoundQueryProgressed {
                id,
                result: QueryResult::GetRecord(GetRecordResult::FoundRecord(record)),
            };
            self.queued_events
                .push_back(NodeEvent::GenerateEvent(out_ev));
        }

        id
    }

    // Removes the record locally, only remove if you are the publisher
    pub fn remove_record(&mut self, key: &Key) {
        if let Some(record) = self.store.get(key) {
            if record.publisher.as_ref() == Some(self.local_key()) {
                self.store.remove(key);

                self.queued_events
                    .push_back(NodeEvent::GenerateEvent(OutEvent::StoreChanged(
                        StoreChangedEvent::RemoveRecord {
                            record_key: key.clone(),
                        },
                    )))
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

        self.update_node_status(key.clone(), Some(addr), status);
    }

    pub fn update_node_status(&mut self, target: Key, addr: Option<Multiaddr>, status: NodeStatus) {
        match self.routing_table.update_node_status(target, addr, status) {
            UpdateResult::Pending { disconnected } => {
                if !self.connected_peers.contains(&disconnected) {
                    // Dial the least-recently disconnected node, if it reconnects after dialing
                    // its status will be set to connected and the pending node will be dropped.
                    self.queued_events.push_back(NodeEvent::Dial {
                        peer_id: disconnected,
                    });
                }
            }
            // UpdateResult::Updated => {}
            // UpdateResult::Failed => {}
            _ => {}
        }
    }

    pub fn find_node(&mut self, target: &Key) -> QueryId {
        let peers = self.routing_table.closest_nodes(target);

        let query_info = QueryInfo::FindNode {
            target: target.clone(),
        };
        self.queries.add_query(target.clone(), peers, query_info)
    }

    fn record_received(&mut self, peer_id: Key, mut record: Record, request_id: usize) {
        if record.publisher.as_ref() == Some(self.local_key()) {
            // If the publisher is the local node, the record should not be overwritten
            // in local storage.
            self.queued_events.push_back(NodeEvent::Notify {
                peer_id,
                event: KademliaEvent::PutRecordRes { request_id },
            });
            return;
        }

        let now = Instant::now();

        // Calculate the expiration exponentially inversely proportional to the number of nodes between
        // the local node and the closest node to the record's key. This avoids over-caching outside the
        // K-closest nodes to the record.
        let num_between = self.routing_table.count_nodes_between(&record.key);
        let num_beyond_k = (usize::max(K_VALUE, num_between) - K_VALUE) as u32;
        let expiration = self
            .config
            .record_ttl
            .map(|ttl| now + exp_decrease(ttl, num_beyond_k));
        // The record is stored forever if the local TTL and the records expiration is `None`.
        record.expires = record.expires.or(expiration).min(expiration);

        if !record.is_expired(now) {
            match self.store.put(record.clone()) {
                Ok(()) => {
                    // TODO: only with debug feature
                    self.queued_events
                        .push_back(NodeEvent::GenerateEvent(OutEvent::StoreChanged(
                            StoreChangedEvent::PutRecord { record },
                        )));
                }
                // TODO: error handling
                Err(()) => {}
            }
        }

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

                if let Some(query) = self.queries.get_mut(QueryId(request_id)) {
                    query.on_success(&peer_id, closest_nodes, local_key);
                }
            }
            KademliaEvent::PutRecordReq { record, request_id } => {
                self.record_received(peer_id, record, request_id);
            }
            KademliaEvent::PutRecordRes { request_id } => {
                let local_key = self.local_key().clone();

                if let Some(query) = self.queries.get_mut(QueryId(request_id)) {
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
                let record = match self.store.get(&key) {
                    Some(record) => {
                        if record.is_expired(Instant::now()) {
                            self.store.remove(&key);
                            None
                        } else {
                            Some(record.into_owned())
                        }
                    }
                    None => None,
                };

                let closer_nodes = self.routing_table.closest_nodes(&key);

                self.queued_events.push_back(NodeEvent::Notify {
                    peer_id,
                    event: KademliaEvent::GetRecordRes {
                        record,
                        closer_nodes,
                        request_id,
                    },
                })
            }
            KademliaEvent::GetRecordRes {
                record,
                request_id,
                closer_nodes,
            } => {
                let local_key = self.local_key().clone();

                if let Some(query) = self.queries.get_mut(QueryId(request_id)) {
                    if let QueryInfo::GetRecord {
                        ref mut found_a_record,
                        ..
                    } = query.query_info
                    {
                        if let Some(record) = record {
                            *found_a_record = true;
                            let out_ev = OutEvent::OutBoundQueryProgressed {
                                id: QueryId(request_id),
                                result: QueryResult::GetRecord(GetRecordResult::FoundRecord(
                                    FoundRecord {
                                        source: peer_id,
                                        record,
                                    },
                                )),
                            };
                            self.queued_events
                                .push_back(NodeEvent::GenerateEvent(out_ev));
                        }
                    }

                    query.on_success(&peer_id, closer_nodes, local_key);
                }
            }
            KademliaEvent::Ping { .. } => {}
        }
    }

    fn handle_pool_event(&mut self, ev: PoolEvent) -> Option<NodeEvent> {
        match ev {
            PoolEvent::NewConnection { key, remote_addr } => {
                let endpoint = socketaddr_to_multiaddr(&remote_addr);
                self.connected_peers.insert(key.clone());
                self.update_node_status(key, Some(endpoint), NodeStatus::Connected);

                // Once the connection is established send all the queued events for that peer.
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
            PoolEvent::ConnectionClosed {
                key,
                error,
                remote_addr,
            } => {
                // Set the state for the closed peer to `PeerState::Failed`
                for query in self.queries.iter_mut() {
                    query.on_failure(&key);
                }

                self.connected_peers.remove(&key);
                self.update_node_status(key, None, NodeStatus::Disconnected);

                if let Some(e) = error {
                    eprintln!("Connection with {remote_addr} closed with message: {}", e);
                }
                // TODO: handle disconnect reason
                let out_ev = OutEvent::ConnectionClosed(key);
                return Some(NodeEvent::GenerateEvent(out_ev));
            }
            PoolEvent::ConnectionFailed { error, remote_addr } => {
                eprintln!("Connection with {remote_addr} failed: {error}");
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
        let id = query.id();
        let query_result = query.result();
        match query_result.info {
            QueryInfo::FindNode { target } => {
                let out_ev = OutEvent::OutBoundQueryProgressed {
                    id,
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
                context,
            } => {
                let target = record.key.clone();

                let query_info = QueryInfo::PutRecord {
                    record,
                    quorum,
                    context,
                    step: PutRecordStep::PutRecord {
                        put_success: vec![],
                    },
                };

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
                ..
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
                    id,
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
                        id,
                        result: QueryResult::GetRecord(GetRecordResult::NotFound(key)),
                    };
                    Some(NodeEvent::GenerateEvent(out_ev))
                }
            }
            QueryInfo::Bootstrap { .. } => {
                // TODO: should refresh the furthest away buckets
                // https://github.com/libp2p/rust-libp2p/blob/master/protocols/kad/src/behaviour.rs#L1232-L1301
                let out_ev = OutEvent::OutBoundQueryProgressed {
                    id,
                    result: QueryResult::Bootstrap,
                };
                Some(NodeEvent::GenerateEvent(out_ev))
            }
        }
    }

    fn poll_next_query(&mut self, cx: &mut Context<'_>) -> Poll<NodeEvent> {
        let now = Instant::now();

        // Available capacity for republish/replicate queries.
        let jobs_query_capacity = MAX_QUERIES.saturating_sub(self.queries.size());

        if let Some(mut job) = self.republish_job.take() {
            let max_queries = usize::min(JOB_MAX_NEW_QUERIES, jobs_query_capacity);
            for _ in 0..max_queries {
                if let Poll::Ready(record) = job.poll(cx, &mut self.store, now) {
                    let put_context = if record.publisher.as_ref() == Some(self.local_key()) {
                        PutRecordContext::Republish
                    } else {
                        PutRecordContext::Replicate
                    };
                    let _ = self.publish_record(record, Quorum::All, put_context);
                } else {
                    break;
                }
            }

            self.republish_job = Some(job);
        }

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
                                // The connection thread is not ready to receive an event, we
                                // should try next iteration to send the event again.
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

    #[cfg(feature = "debug")]
    // Return abstract view of routing table
    pub fn get_routing_table(&self) -> HashMap<u8, Vec<Node>> {
        self.routing_table.get_abstract_view()
    }

    #[cfg(feature = "debug")]
    // Return locally stored records
    pub fn get_record_store(&self) -> Vec<&Record> {
        self.store.all_records()
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
        closer_nodes: Vec<Node>,
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
    OutBoundQueryProgressed { id: QueryId, result: QueryResult },
    ConnectionEstablished(Key),
    ConnectionClosed(Key),
    StoreChanged(StoreChangedEvent),
    Other,
}

#[derive(Debug, Clone)]
pub enum StoreChangedEvent {
    PutRecord { record: Record },
    RemoveRecord { record_key: Key },
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
    FoundRecord(FoundRecord),
    NotFound(Key),
}

#[derive(Debug, Clone)]
pub struct FoundRecord {
    pub source: Key,
    pub record: Record,
}

#[derive(Debug, Clone)]
pub struct NoKnownPeers;

impl fmt::Display for NoKnownPeers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "No known peers.")
    }
}

impl std::error::Error for NoKnownPeers {}

/// Exponentially decrease the given duration (base 2).
/// https://github.com/libp2p/rust-libp2p/blob/master/protocols/kad/src/behaviour.rs#L2054
fn exp_decrease(ttl: Duration, exp: u32) -> Duration {
    Duration::from_secs(ttl.as_secs().checked_shr(exp).unwrap_or(0))
}
