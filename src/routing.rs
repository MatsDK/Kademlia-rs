use arrayvec::ArrayVec;
use multiaddr::Multiaddr;
use serde::{Deserialize, Serialize};

use crate::{
    key::{Distance, Key},
    K_VALUE,
};

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
    // nodes: ArrayVec<(Key, Multiaddr), { K_VALUE }>,
    nodes: ArrayVec<Node, { K_VALUE }>,
}

impl KBucket {
    fn new() -> Self {
        Self {
            nodes: ArrayVec::new(),
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

    fn insert(&mut self, node: Node) {
        // TODO: If the bucket is full we should check if there is a disconnected node
        if self.nodes.is_full() {
            return;
        }

        self.nodes.push(node);
    }

    fn update(&mut self, key: Key, status: NodeStatus) {
        if let Some(mut node) = self.remove(key) {
            node.status = status;
            self.insert(node);
        }
    }
}

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
                        bucket.insert(Node {
                            key: target,
                            addr,
                            status: new_status,
                        })
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
