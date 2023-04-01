use std::time::{Duration, Instant};

use arrayvec::ArrayVec;
use multiaddr::Multiaddr;
use serde::{Deserialize, Serialize};

use crate::{
    key::{Distance, Key},
    K_VALUE,
};

#[derive(Debug)]
pub struct RoutingTable {
    pub local_key: Key,
    kbuckets: Vec<KBucket>,
}

impl RoutingTable {
    pub fn new(key: Key) -> Self {
        Self {
            kbuckets: (0..256).map(|_| KBucket::new()).collect(),
            local_key: key,
        }
    }

    pub fn update_node_status(
        &mut self,
        target: Key,
        addr: Option<Multiaddr>,
        new_status: NodeStatus,
    ) {
        let d = &self.local_key.distance(&target);
        let bucket_idx = BucketIndex::new(d);

        if let Some(i) = bucket_idx {
            let bucket = &mut self.kbuckets[i.index()];

            match bucket.get_node(target) {
                Some(node) => {
                    if node.status != new_status {
                        bucket.update(target, new_status);
                    }
                }
                None => {
                    if let Some(addr) = addr {
                        match bucket.insert(Node {
                            key: target,
                            addr,
                            status: new_status,
                        }) {
                            InsertResult::Inserted => {}
                            InsertResult::Pending { disconnected } => {}
                            InsertResult::Full => {}
                        }
                    }
                }
            }
        } else {
            eprintln!("SelfEntry");
        }
    }

    pub fn closest_nodes(&mut self, target: &Key) -> Vec<Node> {
        let mut closest = Vec::new();

        let d = self.local_key.distance(target);
        let mut bucket_idx = BucketIndex::new(&d);

        if bucket_idx.is_none() {
            // eprintln!("Self lookup");
            bucket_idx = Some(BucketIndex(0));
            // return closest;
        }

        let index = bucket_idx.unwrap().index();

        closest.append(&mut self.kbuckets[index].get_nodes());

        // Look for the closest on the left if less than K closest found
        if closest.len() < K_VALUE {
            for i in (0..index).rev() {
                closest.append(&mut self.kbuckets[i].get_nodes());
                if closest.len() >= K_VALUE {
                    break;
                }
            }
        }

        // Now look for the closest on the right if less than K closest found
        if closest.len() < K_VALUE {
            for i in (index + 1)..256 {
                closest.append(&mut self.kbuckets[i].get_nodes());
                if closest.len() >= K_VALUE {
                    break;
                }
            }
        }

        // Sort by distance to local_key
        // closest.sort_by(|a, b| self.local_key.distance(a).cmp(&self.local_key.distance(b)));
        closest.sort_by_key(|Node { key, .. }| self.local_key.distance(key));

        if closest.len() > K_VALUE {
            return closest[..K_VALUE].to_vec();
        }

        closest
    }

    pub fn get_addr(&self, peer: &Key) -> Option<&Multiaddr> {
        let d = self.local_key.distance(peer);
        let bucket_idx = BucketIndex::new(&d);

        if let Some(i) = bucket_idx {
            let bucket = &self.kbuckets[i.index()];

            return bucket
                .nodes
                .iter()
                .find(|Node { key, .. }| key == peer)
                .map(|Node { addr, .. }| addr);
        }

        None
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct BucketIndex(usize);

impl BucketIndex {
    fn new(d: &Distance) -> Option<BucketIndex> {
        d.ilog2().map(|idx| BucketIndex(idx as usize))
        // d.leading_zeros().map(|idx| BucketIndex(idx as usize))
    }

    fn index(&self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeStatus {
    Connected,
    Disconnected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub key: Key,
    pub addr: Multiaddr,
    pub status: NodeStatus,
}

#[derive(Debug)]
struct KBucket {
    // The nodes stored in the bucket ordered from least-recently connected to
    // most-recentrly connected.
    nodes: ArrayVec<Node, { K_VALUE }>,

    // The index in `nodes` that indicates the position of the first connected node.
    //
    // All nodes above this index are considered connected, i.e. the range
    // `[0, first_connected_idx)` indicates the range of nodes that are
    // considered disconnected and the range, `[first_connected_idx, K_VALUE)`
    // is the range of nodes that are considered connected
    first_connected_idx: Option<usize>,

    // A node that is waiting to be possibely inserted into a full bucket
    pending: Option<PendingNode>,

    // The duration a pending node should wait before being inserted into a full bucket.
    // During this timeout the least-recently connected node may re-connect.
    pending_timeout: Duration,
}

#[derive(Debug)]
struct PendingNode {
    // The node pending to be inserted.
    node: Node,

    // The instant at which it is OK to insert into the bucket.
    insert_instant: Instant,
}

pub enum InsertResult {
    Inserted,
    Pending { disconnected: Key },
    Full,
}

impl KBucket {
    fn new() -> Self {
        Self {
            nodes: ArrayVec::new(),
            first_connected_idx: None,
            pending: None,
            pending_timeout: Duration::from_secs(10),
        }
    }

    fn get_keys(&self) -> Vec<Key> {
        self.nodes
            .iter()
            .map(|Node { key, .. }| key.clone())
            .collect()
    }

    fn get_nodes(&self) -> Vec<Node> {
        self.nodes.to_vec()
    }

    fn get_node(&self, key: Key) -> Option<&Node> {
        self.nodes.iter().find(|n| n.key == key)
    }

    fn get_index(&self, key: Key) -> Option<usize> {
        self.nodes.iter().position(|n| n.key == key)
    }

    fn remove(&mut self, key: Key) -> Option<Node> {
        if let Some(i) = self.get_index(key) {
            let node = self.nodes.remove(i);

            Some(node)
        } else {
            None
        }
    }

    fn insert(&mut self, node: Node) -> InsertResult {
        match node.status {
            NodeStatus::Connected => {
                if self.nodes.is_full() {
                    // All nodes in this bucket are considered connected.
                    if self.first_connected_idx == Some(0) {
                        return InsertResult::Full;
                    } else {
                        self.pending = Some(PendingNode {
                            node,
                            insert_instant: Instant::now() + self.pending_timeout,
                        });
                        return InsertResult::Pending {
                            disconnected: self.nodes[0].key,
                        };
                    }
                }

                // If there is no connected node in the bucket, set `first_connected_idx` to the
                // last element in the list.
                self.first_connected_idx.or(Some(self.nodes.len()));
                self.nodes.push(node);
                InsertResult::Inserted
            }
            NodeStatus::Disconnected => {
                if self.nodes.is_full() {
                    return InsertResult::Full;
                }

                // The bucket is not full so insert before the first connected node and increase
                // `first_connected_idx` by 1.
                if let Some(ref mut first_connected_idx) = self.first_connected_idx {
                    self.nodes.insert(*first_connected_idx, node);
                    *first_connected_idx += 1;
                } else {
                    self.nodes.push(node);
                }

                InsertResult::Inserted
            }
        }
    }

    fn update(&mut self, key: Key, status: NodeStatus) {
        if let Some(mut node) = self.remove(key) {
            node.status = status;
            self.insert(node);
        }
    }
}
