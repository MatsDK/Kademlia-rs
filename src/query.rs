use std::collections::HashMap;

use crate::{key::Key, node::KademliaEvent, K_VALUE};

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

    pub fn on_success(&mut self, peer: &Key, closer_peers: Vec<Key>) {
        self.peers_iter.on_success(peer, closer_peers);
    }

    pub fn next(&mut self) -> QueryState {
        self.peers_iter.next()
    }
}

#[derive(Debug)]
enum PeersIterState {
    Finished,
    Iterating,
    Waiting(Option<Key>),
}

#[derive(Debug)]
pub struct PeersIter {
    state: PeersIterState,
    target: Key,
    closest_peers: Vec<Peer>,
    num_waiting: usize,
    num_results: usize,
}

impl PeersIter {
    pub fn new(target: Key, closest_peers: Vec<Key>) -> Self {
        let closest_peers = closest_peers
            .into_iter()
            .map(|key| Peer {
                key,
                state: PeerState::NotContacted,
            })
            .collect();

        Self {
            state: PeersIterState::Iterating,
            target,
            closest_peers,
            num_waiting: 0,
            num_results: K_VALUE,
        }
    }

    pub fn on_success(&mut self, peer_id: &Key, closer_peers: Vec<Key>) {
        // println!("Got response from {} with {:?}", peer_id, closer_peers);
        let peer = self
            .closest_peers
            .iter_mut()
            .find(|peer| peer.key == *peer_id);

        if let Some(mut peer) = peer {
            match peer.state {
                PeerState::Waiting => {
                    self.num_waiting -= 1;
                    peer.state = PeerState::Succeeded
                }
                PeerState::Unresponsive => peer.state = PeerState::Succeeded,
                PeerState::NotContacted | PeerState::Succeeded | PeerState::Failed => {}
            }
        }
    }

    pub fn next(&mut self) -> QueryState {
        if let PeersIterState::Finished = self.state {
            return QueryState::Finished;
        }

        let mut result_counter = 0;

        for peer in self.closest_peers.iter_mut() {
            match peer.state {
                PeerState::NotContacted => {
                    // TODO: check if 'self.num_waiting' is below some maximum

                    self.num_waiting += 1;
                    peer.state = PeerState::Waiting;
                    return QueryState::Waiting(Some(peer.key.clone()));
                }
                PeerState::Waiting => {
                    // TODO: check for timeout
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
