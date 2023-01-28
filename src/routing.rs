use arrayvec::ArrayVec;
use multiaddr::Multiaddr;

use crate::{
    key::{Distance, Key},
    K_VALUE,
};

#[derive(Debug)]
struct KBucket {
    nodes: ArrayVec<(Key, Multiaddr), { K_VALUE }>,
}

impl KBucket {
    fn new() -> Self {
        Self {
            nodes: ArrayVec::new(),
        }
    }

    fn get_keys(&self) -> Vec<Key> {
        self.nodes.iter().map(|(key, _)| key.clone()).collect()
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

    pub fn insert(&mut self, target: &Key, addr: Multiaddr) {
        let d = &self.local_key.distance(target);
        let bucket_idx = BucketIndex::new(d);

        if let Some(i) = bucket_idx {
            let bucket = &mut self.kbuckets[i.index()];

            // If the bucket is full we should check if there is a disconnected node
            if bucket.nodes.is_full() {
                return;
            }

            bucket.nodes.push((target.clone(), addr));
        } else {
            eprintln!("SelfEntry");
        }
    }

    pub fn closest_nodes(&mut self, target: &Key) -> Vec<Key> {
        let mut closest = Vec::new();

        let d = self.local_key.distance(target);
        let bucket_idx = BucketIndex::new(&d);

        if bucket_idx.is_none() {
            eprintln!("SelfEntry");
            return closest;
        }

        let index = bucket_idx.unwrap().index();

        closest.append(&mut self.kbuckets[index].get_keys());

        // Look for the closest on the left if less than K closest found
        if closest.len() < K_VALUE {
            for i in (0..index).rev() {
                closest.append(&mut self.kbuckets[i].get_keys());
                if closest.len() >= K_VALUE {
                    break;
                }
            }
        }

        // Now look for the closest on the right if less than K closest found
        if closest.len() < K_VALUE {
            for i in (index + 1)..256 {
                closest.append(&mut self.kbuckets[i].get_keys());
                if closest.len() >= K_VALUE {
                    break;
                }
            }
        }

        // Sort by distance to local_key
        // closest.sort_by(|a, b| self.local_key.distance(a).cmp(&self.local_key.distance(b)));
        closest.sort_by_key(|a| self.local_key.distance(a));

        if closest.len() > K_VALUE {
            return closest[..K_VALUE].to_vec();
        }

        closest
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
