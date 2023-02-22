use std::collections::{btree_map::Entry, BTreeMap, HashMap};

use crate::{
    key::{Distance, Key},
    node::KademliaEvent,
    K_VALUE,
};

#[derive(Debug)]
pub enum QueryPoolState<'a> {
    Waiting(Option<(&'a Query, Key)>),
    Finished(Query),
    Idle,
}

#[derive(Debug)]
pub struct QueryPool {
    next_query_id: usize,
    queries: HashMap<usize, Query>,
}

impl QueryPool {
    pub fn new() -> Self {
        Self {
            next_query_id: 0,
            queries: HashMap::new(),
        }
    }

    pub fn add_query(&mut self, target: Key, peers: Vec<Key>, query_id: usize, ev: KademliaEvent) {
        if peers.is_empty() {
            return;
        }

        let peers_iter = PeersIter::new(target, peers);
        let query = Query::new(peers_iter, ev, query_id);
        self.queries.insert(query_id, query);
    }

    pub fn next_query_id(&mut self) -> usize {
        let id = self.next_query_id;
        self.next_query_id = self.next_query_id.wrapping_add(1);
        id
    }

    pub fn get_mut(&mut self, query_id: &usize) -> Option<&mut Query> {
        self.queries.get_mut(query_id)
    }

    pub fn poll(&mut self) -> QueryPoolState<'_> {
        // TODO: handle timeout events

        let mut waiting = None;
        let mut finished = None;

        for (&query_id, query) in self.queries.iter_mut() {
            match query.next() {
                QueryState::Waiting(Some(peer)) => {
                    waiting = Some((query_id, peer));
                    break;
                }
                QueryState::Finished => {
                    finished = Some(query_id);
                    break;
                }
                QueryState::Waiting(None) | QueryState::Failed => {}
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
    Failed,
}

#[derive(Debug)]
pub struct Query {
    peers_iter: PeersIter,
    event: KademliaEvent,
    id: usize,
}

impl Query {
    pub fn new(peers_iter: PeersIter, event: KademliaEvent, query_id: usize) -> Self {
        Self {
            peers_iter,
            event,
            id: query_id,
        }
    }

    pub fn id(&self) -> usize {
        self.id
    }

    pub fn get_event(&self) -> KademliaEvent {
        self.event.clone()
    }

    pub fn on_success(&mut self, peer: &Key, closer_peers: Vec<Key>, local_key: Key) {
        let other_peers = closer_peers
            .into_iter()
            .filter(|p| p != &local_key)
            .collect();
        self.peers_iter.on_success(peer, other_peers);
    }

    pub fn next(&mut self) -> QueryState {
        self.peers_iter.next()
    }
}

#[derive(Debug)]
enum PeersIterState {
    Finished,
    Iterating,
    Stalled,
}

#[derive(Debug)]
pub struct PeersIter {
    state: PeersIterState,
    target: Key,
    closest_peers: BTreeMap<Distance, Peer>,
    num_waiting: usize,
    num_results: usize,
}

impl PeersIter {
    pub fn new(target: Key, closest_peers: Vec<Key>) -> Self {
        let closest_peers = BTreeMap::from_iter(closest_peers.into_iter().map(|key| {
            let distance = key.distance(&target);
            (
                distance,
                Peer {
                    key,
                    state: PeerState::NotContacted,
                },
            )
        }));
        Self {
            state: PeersIterState::Iterating,
            target,
            closest_peers,
            num_waiting: 0,
            num_results: K_VALUE,
        }
    }

    pub fn on_success(&mut self, peer_id: &Key, closer_peers: Vec<Key>) -> bool {
        if let PeersIterState::Finished = self.state {
            return false;
        }

        let distance = peer_id.distance(&self.target);
        // Update the state of the resolved peer
        match self.closest_peers.entry(distance) {
            Entry::Vacant(..) => return false,
            Entry::Occupied(mut e) => match e.get().state {
                PeerState::Waiting => {
                    self.num_waiting -= 1;
                    e.get_mut().state = PeerState::Succeeded;
                }
                PeerState::Unresponsive => {
                    e.get_mut().state = PeerState::Succeeded;
                }
                PeerState::NotContacted | PeerState::Failed | PeerState::Succeeded => return false,
            },
        }

        // Add `closer_peers` to the iterator
        for key in closer_peers.into_iter() {
            let distance = self.target.distance(&key);
            let new_peer = Peer {
                key,
                state: PeerState::NotContacted,
            };

            self.closest_peers.entry(distance).or_insert(new_peer);
        }

        // Set new iterator state, check if iteration stalled based on
        // the `ALPHA_VALUE` (Parallelism) and if the iterator made progres
        // self.state = match self.state {
        //     PeersIterState::Iterating {
        //     }
        // }
        true
    }

    pub fn next(&mut self) -> QueryState {
        if let PeersIterState::Finished = self.state {
            return QueryState::Finished;
        }

        let mut result_counter = 0;

        for peer in self.closest_peers.values_mut() {
            match peer.state {
                PeerState::NotContacted => {
                    // TODO: check if 'self.num_waiting' is below some maximum

                    self.num_waiting += 1;
                    peer.state = PeerState::Waiting;
                    return QueryState::Waiting(Some(peer.key.clone()));
                }
                PeerState::Waiting => {
                    // TODO: check for timeout, set to unresponsive
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
    Waiting,
    Succeeded,
    Unresponsive,
    Failed,
}

#[derive(Debug)]
struct Peer {
    key: Key,
    state: PeerState,
}
