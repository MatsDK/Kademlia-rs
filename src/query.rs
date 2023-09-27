use std::{
    collections::{btree_map::Entry, BTreeMap, HashMap},
    num::NonZeroUsize,
    time::{Duration, Instant},
};

use multiaddr::Multiaddr;

use crate::{
    key::{Distance, Key},
    node::KademliaEvent,
    routing::Node,
    store::Record,
    ALPHA_VALUE, K_VALUE,
};

#[derive(Debug)]
pub enum QueryPoolState<'a> {
    Waiting(Option<(&'a mut Query, Key)>),
    Finished(Query),
    TimeOut(Query),
    Idle,
}

#[derive(Debug)]
pub struct QueryPool {
    next_query_id: usize,
    queries: HashMap<QueryId, Query>,
    query_timeout: Duration,
}

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct QueryId(pub usize);

impl QueryPool {
    pub fn new() -> Self {
        Self {
            next_query_id: 0,
            queries: HashMap::new(),
            query_timeout: Duration::from_secs(60),
        }
    }

    pub fn size(&self) -> usize {
        self.queries.len()
    }

    pub fn add_query(&mut self, target: Key, peers: Vec<Node>, info: QueryInfo) -> QueryId {
        let query_id = self.next_query_id();
        self.add_query_with_id(query_id, target, peers, info);
        query_id
    }

    pub fn add_query_with_id(
        &mut self,
        query_id: QueryId,
        target: Key,
        peers: Vec<Node>,
        info: QueryInfo,
    ) {
        assert!(!self.queries.contains_key(&query_id));
        if peers.is_empty() {
            return;
        }

        let peers_iter = PeersIter::new(target, peers);
        let query = Query::new(peers_iter, info, query_id);
        self.queries.insert(query_id, query);
    }

    pub fn next_query_id(&mut self) -> QueryId {
        let id = QueryId(self.next_query_id);
        self.next_query_id = self.next_query_id.wrapping_add(1);
        id
    }

    pub fn get_mut(&mut self, query_id: QueryId) -> Option<&mut Query> {
        self.queries.get_mut(&query_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Query> {
        self.queries.values()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Query> {
        self.queries.values_mut()
    }

    pub fn poll(&mut self) -> QueryPoolState<'_> {
        let now = Instant::now();

        let mut waiting = None;
        let mut finished = None;
        let mut timeout = None;

        for (&query_id, query) in self.queries.iter_mut() {
            match query.next(now) {
                QueryState::Waiting(Some(peer)) => {
                    waiting = Some((query_id, peer));
                    break;
                }
                QueryState::Finished => {
                    finished = Some(query_id);
                    break;
                }
                QueryState::Waiting(None) | QueryState::WaitingAtCapacity => {
                    let time_elapsed = now - query.start;
                    if time_elapsed >= self.query_timeout {
                        timeout = Some(query_id);
                        break;
                    }
                }
            }
        }

        if let Some((query_id, peer)) = waiting {
            let query = self.queries.get_mut(&query_id).expect("Query not found");
            return QueryPoolState::Waiting(Some((query, peer)));
        }

        if let Some(query_id) = finished {
            let query = self.queries.remove(&query_id).expect("Query not found");
            return QueryPoolState::Finished(query);
        }

        if let Some(query_id) = timeout {
            let query = self.queries.remove(&query_id).expect("Query not found");
            return QueryPoolState::TimeOut(query);
        }

        if self.queries.is_empty() {
            QueryPoolState::Idle
        } else {
            QueryPoolState::Waiting(None)
        }
    }
}

pub enum QueryState {
    Finished,
    Waiting(Option<Key>),
    WaitingAtCapacity,
}

#[derive(Debug)]
pub struct Query {
    pub query_info: QueryInfo,
    pub waiting_events: HashMap<Key, Vec<KademliaEvent>>,
    peers_iter: PeersIter,
    id: QueryId,
    discovered_addrs: HashMap<Key, Multiaddr>,
    start: Instant,
}

#[derive(Debug)]
pub struct QueryResultOutput {
    pub query_id: QueryId,
    pub nodes: Vec<Node>,
    pub info: QueryInfo,
}

impl QueryResultOutput {
    pub fn keys(self) -> Vec<Key> {
        self.nodes.into_iter().map(|Node { key, .. }| key).collect()
    }
}

#[derive(Debug)]
pub enum PutRecordStep {
    // Finding nodes responsible for storing the record
    FindNodes,
    // Sending the record to the K nodes closest to the records key
    PutRecord { put_success: Vec<Key> },
}

#[derive(Debug)]
pub enum PutRecordContext {
    Publish,
    Republish,
    Replicate,
}

#[derive(Debug)]
pub enum Quorum {
    One,
    N(NonZeroUsize),
    All,
}

impl Into<NonZeroUsize> for Quorum {
    fn into(self) -> NonZeroUsize {
        match self {
            Quorum::One => NonZeroUsize::new(1).expect("Quorum is set to one"),
            Quorum::N(n) => NonZeroUsize::min(NonZeroUsize::new(K_VALUE).expect("K_VALUE > 0"), n),
            Quorum::All => NonZeroUsize::new(K_VALUE).expect("K_VALUE > 0"),
        }
    }
}

#[derive(Debug)]
pub enum QueryInfo {
    FindNode {
        target: Key,
    },
    PutRecord {
        record: Record,
        step: PutRecordStep,
        context: PutRecordContext,
        quorum: NonZeroUsize,
    },
    GetRecord {
        key: Key,
        found_a_record: bool,
    },
    Bootstrap {
        target: Key,
    },
}

// impl Into<KademliaEvent> for QueryInfo {
// convert from `QueryInfo` to `KademliaEvent` for sending requests
impl QueryInfo {
    fn into_request(&self, query_id: QueryId) -> KademliaEvent {
        let request_id = query_id.0;
        match self {
            QueryInfo::FindNode { target } => KademliaEvent::FindNodeReq {
                target: target.clone(),
                request_id,
            },
            QueryInfo::PutRecord { record, step, .. } => match step {
                PutRecordStep::FindNodes => KademliaEvent::FindNodeReq {
                    target: record.key,
                    request_id,
                },
                PutRecordStep::PutRecord { .. } => KademliaEvent::PutRecordReq {
                    record: record.clone(),
                    request_id,
                },
            },
            QueryInfo::Bootstrap { target } => KademliaEvent::FindNodeReq {
                target: target.clone(),
                request_id,
            },
            QueryInfo::GetRecord { key, .. } => KademliaEvent::GetRecordReq {
                key: *key,
                request_id,
            },
        }
    }
}

impl Query {
    pub fn new(peers_iter: PeersIter, info: QueryInfo, query_id: QueryId) -> Self {
        Self {
            peers_iter,
            id: query_id,
            discovered_addrs: Default::default(),
            waiting_events: Default::default(),
            query_info: info,
            start: Instant::now(),
        }
    }

    pub fn id(&self) -> QueryId {
        self.id
    }

    pub fn get_request(&self) -> KademliaEvent {
        self.query_info.into_request(self.id)
    }

    pub fn get_addr_for_peer(&self, peer: &Key) -> Option<&Multiaddr> {
        self.discovered_addrs.get(peer)
    }

    pub fn get_waiting_events(&mut self, peer: &Key) -> Option<Vec<KademliaEvent>> {
        self.waiting_events.remove(peer)
    }

    pub fn result(self) -> QueryResultOutput {
        let nodes = self.peers_iter.get_peers();

        QueryResultOutput {
            nodes,
            info: self.query_info,
            query_id: self.id,
        }
    }

    pub fn finish(&mut self) {
        self.peers_iter.finish();
    }

    pub fn on_success(&mut self, peer: &Key, closer_peers: Vec<Node>, local_key: Key) {
        let other_peers = closer_peers
            .into_iter()
            .filter(|Node { key, .. }| key != &local_key)
            .map(|node| {
                self.discovered_addrs
                    .insert(node.key.clone(), node.addr.clone());
                node
            })
            .collect();

        self.peers_iter.on_success(peer, other_peers);
    }

    pub fn on_failure(&mut self, peer: &Key) {
        self.peers_iter.on_failure(peer);
    }

    pub fn next(&mut self, now: Instant) -> QueryState {
        self.peers_iter.next(now)
    }
}

#[derive(Debug)]
#[allow(unused)]
enum PeersIterState {
    Finished,
    Iterating { no_progress: usize },
    Stalled,
}

#[derive(Debug)]
pub struct PeersIter {
    state: PeersIterState,
    target: Key,
    closest_peers: BTreeMap<Distance, Peer>,
    num_waiting: usize,
    num_results: usize,
    peer_timeout: Duration,
}

impl PeersIter {
    pub fn new(target: Key, closest_peers: Vec<Node>) -> Self {
        let closest_peers = BTreeMap::from_iter(closest_peers.into_iter().map(|node| {
            let distance = node.key.distance(&target);
            (
                distance,
                Peer {
                    key: node.key,
                    state: PeerState::NotContacted,
                    node,
                },
            )
        }));

        Self {
            state: PeersIterState::Iterating { no_progress: 0 },
            target,
            closest_peers,
            num_waiting: 0,
            num_results: K_VALUE,
            peer_timeout: Duration::from_secs(10),
        }
    }

    pub fn finish(&mut self) {
        self.state = PeersIterState::Finished;
    }

    /// Checks if the iterator is at parallelism capacity.
    fn max_parallelism_reached(&self) -> bool {
        match self.state {
            PeersIterState::Stalled => {
                self.num_waiting >= usize::max(self.num_results, ALPHA_VALUE)
            }
            PeersIterState::Iterating { .. } => self.num_waiting >= ALPHA_VALUE,
            PeersIterState::Finished => true,
        }
    }

    pub fn get_peers(self) -> Vec<Node> {
        self.closest_peers
            .into_iter()
            .filter_map(|(_, peer)| {
                if let PeerState::Succeeded = peer.state {
                    Some(peer.node)
                } else {
                    None
                }
            })
            .take(self.num_results)
            .collect()
    }

    pub fn on_success(&mut self, peer_id: &Key, closer_peers: Vec<Node>) {
        if let PeersIterState::Finished = self.state {
            return;
        }

        let distance = peer_id.distance(&self.target);
        // Update the state of the resolved peer
        match self.closest_peers.entry(distance) {
            Entry::Vacant(..) => return,
            Entry::Occupied(mut e) => match e.get().state {
                PeerState::Waiting(_) => {
                    self.num_waiting -= 1;
                    e.get_mut().state = PeerState::Succeeded;
                }
                PeerState::Unresponsive => {
                    e.get_mut().state = PeerState::Succeeded;
                }
                PeerState::NotContacted | PeerState::Failed | PeerState::Succeeded => return,
            },
        }

        let num_closest = self.closest_peers.len();
        let mut made_progress = false;

        // Add `closer_peers` to the iterator
        for node in closer_peers.into_iter() {
            let key = node.key;
            let distance = self.target.distance(&key);
            let new_peer = Peer {
                key,
                state: PeerState::NotContacted,
                node,
            };

            made_progress = self.closest_peers.keys().next() == Some(&distance)
                || num_closest < self.num_results;

            self.closest_peers.entry(distance).or_insert(new_peer);
        }

        // Update the state, check if the iterator stalled.
        self.state = match self.state {
            PeersIterState::Stalled => {
                if made_progress {
                    PeersIterState::Iterating { no_progress: 0 }
                } else {
                    PeersIterState::Stalled
                }
            }
            PeersIterState::Iterating { no_progress } => {
                let no_progress = if made_progress { 0 } else { no_progress + 1 };
                if no_progress >= ALPHA_VALUE {
                    PeersIterState::Stalled
                } else {
                    PeersIterState::Iterating { no_progress }
                }
            }
            PeersIterState::Finished => PeersIterState::Finished,
        };
    }

    fn on_failure(&mut self, peer: &Key) {
        if let PeersIterState::Finished = self.state {
            return;
        }

        let distance = peer.distance(&self.target);
        // Update the state of the failed peer
        match self.closest_peers.entry(distance) {
            Entry::Occupied(mut e) => match e.get().state {
                PeerState::Waiting(_) => {
                    self.num_waiting -= 1;
                    e.get_mut().state = PeerState::Failed;
                }
                PeerState::Unresponsive => e.get_mut().state = PeerState::Failed,
                PeerState::NotContacted | PeerState::Failed | PeerState::Succeeded => {}
            },
            Entry::Vacant(..) => {}
        }
    }

    pub fn next(&mut self, now: Instant) -> QueryState {
        if let PeersIterState::Finished = self.state {
            return QueryState::Finished;
        }

        let mut result_counter = 0;

        // Check if the iterator is at its parallelism capacity.
        let at_capacity = self.max_parallelism_reached();

        for peer in self.closest_peers.values_mut() {
            match peer.state {
                PeerState::NotContacted => {
                    if at_capacity {
                        return QueryState::WaitingAtCapacity;
                    }

                    self.num_waiting += 1;
                    let timeout = now + self.peer_timeout;
                    peer.state = PeerState::Waiting(timeout);
                    return QueryState::Waiting(Some(peer.key.clone()));
                }
                PeerState::Waiting(timeout) => {
                    if now >= timeout {
                        self.num_waiting -= 1;
                        peer.state = PeerState::Unresponsive
                    } else if at_capacity {
                        return QueryState::WaitingAtCapacity;
                    }
                }
                PeerState::Succeeded => {
                    result_counter += 1;

                    if result_counter >= self.num_results {
                        self.state = PeersIterState::Finished;
                        return QueryState::Finished;
                    }
                }
                PeerState::Unresponsive | PeerState::Failed => {}
            }
        }

        if self.num_waiting > 0 {
            // The iterator is still waiting for some peers to respond or to timeout
            QueryState::Waiting(None)
        } else {
            self.state = PeersIterState::Finished;
            QueryState::Finished
        }
    }
}

#[derive(Debug)]
enum PeerState {
    NotContacted,
    Waiting(Instant),
    Succeeded,
    Unresponsive,
    Failed,
}

#[derive(Debug)]
struct Peer {
    key: Key,
    state: PeerState,
    node: Node,
}
