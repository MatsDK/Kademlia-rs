use std::collections::HashMap;

use crate::{key::Key, node::KademliaEvent};

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

    pub fn add_query(&mut self, peers: Vec<Key>, query_id: usize, ev: KademliaEvent) {
        if peers.len() == 0 {
            return;
        }

        // let query_id = self.next_query_id();
        let query = Query::new(peers, ev, query_id);
        self.queries.insert(query_id, query);
    }

    pub fn next_query_id(&mut self) -> usize {
        let id = self.next_query_id.clone();
        self.next_query_id = self.next_query_id.wrapping_add(1);
        id
    }

    pub fn poll(&mut self) -> QueryPoolState<'_> {
        let mut waiting = None;
        let mut finished = None;

        for (&query_id, query) in self.queries.iter_mut() {
            match query.next() {
                QueryState::Waiting(peer) => {
                    waiting = Some((query_id, peer));
                    break;
                }
                QueryState::Finished => {
                    finished = Some(query_id);
                    break;
                }
                QueryState::Failed => {
                    println!("Query failed");
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

        if self.queries.is_empty() {
            QueryPoolState::Idle
        } else {
            QueryPoolState::Waiting(None)
        }
    }
}

pub enum QueryState {
    Finished,
    Waiting(Key),
    Failed,
}

#[derive(Debug)]
pub struct Query {
    peers: Vec<Key>,
    event: KademliaEvent,
    count: usize,
    id: usize,
}

impl Query {
    pub fn new(peers: Vec<Key>, event: KademliaEvent, query_id: usize) -> Self {
        Self {
            peers,
            event,
            count: 0,
            id: query_id,
        }
    }

    pub fn id(&self) -> usize {
        self.id
    }

    pub fn get_event(&self) -> KademliaEvent {
        self.event.clone()
    }

    pub fn next(&mut self) -> QueryState {
        if self.count >= self.peers.len() {
            return QueryState::Finished;
        }
        self.count += 1;

        QueryState::Waiting(self.peers[self.count - 1].clone())
    }
}
