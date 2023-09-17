use arrayvec::ArrayVec;
use multiaddr::Multiaddr;
use serde::{Deserialize, Serialize};
#[cfg(feature = "debug")]
use std::collections::HashMap;
use std::{
    fmt,
    time::{Duration, Instant},
};

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
    ) -> UpdateResult {
        let d = &self.local_key.distance(&target);
        let bucket_idx = BucketIndex::new(d);

        if let Some(i) = bucket_idx {
            let bucket = &mut self.kbuckets[i.index()];
            bucket.apply_pending();

            match bucket.get_node(target) {
                Some(node) => {
                    if node.status != new_status {
                        bucket.update(target, new_status);
                        return UpdateResult::Updated;
                    }
                }
                None => {
                    if let Some(addr) = addr {
                        return match bucket.insert(Node {
                            key: target,
                            addr,
                            status: new_status,
                        }) {
                            InsertResult::Inserted => UpdateResult::Updated,
                            InsertResult::Pending { disconnected } => {
                                UpdateResult::Pending { disconnected }
                            }
                            InsertResult::Full => UpdateResult::Failed,
                        };
                    }
                }
            }
        } else {
            eprintln!("SelfEntry");
        }

        UpdateResult::Failed
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
        let bucket = &mut self.kbuckets[index];
        bucket.apply_pending();

        closest.append(&mut bucket.get_nodes());

        // Look for the closest on the left if less than K closest found
        if closest.len() < K_VALUE {
            for i in (0..index).rev() {
                let bucket = &mut self.kbuckets[i];
                bucket.apply_pending();

                closest.append(&mut bucket.get_nodes());
                if closest.len() >= K_VALUE {
                    break;
                }
            }
        }

        // Now look for the closest on the right if less than K closest found
        if closest.len() < K_VALUE {
            for i in (index + 1)..256 {
                let bucket = &mut self.kbuckets[i];
                bucket.apply_pending();

                closest.append(&mut bucket.get_nodes());
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

    pub fn count_nodes_between(&self, target: &Key) -> usize {
        let distance = target.distance(&self.local_key);
        let mut count = 0;

        let mut bucket_idx = BucketIndex::new(&distance);

        if bucket_idx.is_none() {
            bucket_idx = Some(BucketIndex(0));
        }

        // First, count closer nodes in the target's bucket
        let index = bucket_idx.unwrap().index();
        let bucket = &self.kbuckets[index];
        count += bucket
            .get_nodes()
            .iter()
            .filter(|&Node { key, .. }| key.distance(&self.local_key) <= distance)
            .count();

        // Count all other nodes to the left
        count += (0..index)
            .rev()
            .map(|i| self.kbuckets[i].count_nodes())
            .sum::<usize>();

        count
    }

    pub fn get_addr(&self, peer: &Key) -> Option<&Multiaddr> {
        let d = self.local_key.distance(peer);
        let bucket_idx = BucketIndex::new(&d);

        if let Some(i) = bucket_idx {
            let bucket = &self.kbuckets[i.index()];
            // bucket.apply_pending();

            return bucket
                .nodes
                .iter()
                .find(|Node { key, .. }| key == peer)
                .map(|Node { addr, .. }| addr);
        }

        None
    }

    #[cfg(feature = "debug")]
    pub fn get_abstract_view(&self) -> HashMap<u8, Vec<Node>> {
        let mut routing_table = HashMap::new();
        self.kbuckets.iter().enumerate().for_each(|(idx, bucket)| {
            if bucket.nodes.len() == 0 {
                return;
            }

            routing_table.insert(idx as u8, bucket.get_nodes());
        });
        routing_table
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

impl fmt::Display for NodeStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            NodeStatus::Connected => write!(f, "Connected"),
            NodeStatus::Disconnected => write!(f, "Disconnected"),
        }
    }
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

pub enum UpdateResult {
    Updated,
    Pending { disconnected: Key },
    Failed,
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

    #[allow(unused)]
    fn get_keys(&self) -> Vec<Key> {
        self.nodes
            .iter()
            .map(|Node { key, .. }| key.clone())
            .collect()
    }

    fn get_nodes(&self) -> Vec<Node> {
        self.nodes.to_vec()
    }

    fn count_nodes(&self) -> usize {
        self.nodes.len()
    }

    fn get_node(&self, key: Key) -> Option<&Node> {
        self.nodes.iter().find(|n| n.key == key)
    }

    fn get_index(&self, key: Key) -> Option<usize> {
        self.nodes.iter().position(|n| n.key == key)
    }

    fn remove(&mut self, key: Key) -> Option<(Node, usize)> {
        if let Some(i) = self.get_index(key) {
            let node = self.nodes.remove(i);

            // Update `first_connected_idx` according to the status of the removed node.
            match node.status {
                NodeStatus::Connected => {
                    if self.first_connected_idx.map_or(false, |idx| idx == i)
                        && i == self.nodes.len()
                    {
                        // The removed node was the only node considered connected.
                        self.first_connected_idx = None
                    }
                }
                NodeStatus::Disconnected => {
                    if let Some(ref mut idx) = self.first_connected_idx {
                        *idx -= 1;
                    }
                }
            }

            Some((node, i))
        } else {
            None
        }
    }

    // Inserts the pending node into the bucket if the timeout has elapsed, replacing the
    // least-recently connected node, `self.nodes[0]`. This should be called every time this bucket
    // is accessed.
    fn apply_pending(&mut self) {
        let now = Instant::now();
        let Some(pending) = self.pending.take() else {
            return
        };

        if pending.insert_instant > now {
            // The pending node must continue to wait until the timeout has elapsed.
            self.pending = Some(pending);
            return;
        }

        // The node waited for `pending_timeout`, at this point the node can possibely be
        // inserted into the bucket.
        if self.nodes.is_full() {
            if self.nodes[0].status == NodeStatus::Connected {
                // The bucket is completely occupied by connected nodes so drop the pending node.
                return;
            }

            if pending.node.status == NodeStatus::Disconnected {
                let _removed = self.nodes.remove(0);

                // Decrease `first_connected_idx` because a disconnected node is replaced with the
                // connected pending node.
                self.first_connected_idx = self
                    .first_connected_idx
                    .map_or_else(|| Some(self.nodes.len()), |idx| idx.checked_sub(1));

                self.nodes.push(pending.node);
            } else if let Some(idx) = self.first_connected_idx {
                // If the pending node is disconnected, it should be inserted at the end of the
                // disconnected nodes list, `first_connected_idx` - 1
                let _removed = self.nodes.remove(0);

                let insert_idx = idx.checked_sub(1).expect("subtract 1 from index");
                self.nodes.insert(insert_idx, pending.node);
            } else {
                // All nodes in the bucket are disconnected, insert the pending node at the end as
                // the most recently disconnected node.
                let _removed = self.nodes.remove(0);

                self.nodes.push(pending.node);
            }
        } else {
            match self.insert(pending.node) {
                InsertResult::Inserted => {}
                _ => unreachable!("Should be able to insert, bucket is not full"),
            }
        }
    }

    #[allow(unused)]
    pub fn status(&self, pos: usize) -> NodeStatus {
        if self.first_connected_idx.map_or(false, |i| pos >= i) {
            NodeStatus::Connected
        } else {
            NodeStatus::Disconnected
        }
    }

    fn insert(&mut self, node: Node) -> InsertResult {
        match node.status {
            NodeStatus::Connected => {
                if self.nodes.is_full() {
                    if self.first_connected_idx == Some(0) {
                        // All nodes in this bucket are considered connected.
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
        // Remove the node and reinsert it somewhere in the list depending on the new status.
        if let Some((mut node, idx)) = self.remove(key) {
            node.status = status;

            // If the least-recently connected node reconnects, drop the pending node that was
            // waiting for this node to reconnect.
            if idx == 0 && node.status == NodeStatus::Connected {
                self.pending = None;
            }

            self.insert(node);
        }
    }
}
