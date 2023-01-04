use crate::{
    key::{Distance, Key},
    K_VALUE,
};

#[derive(Debug)]
struct KBucket {
    nodes: Vec<Key>,
}

impl KBucket {
    fn new() -> Self {
        Self { nodes: Vec::new() }
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

    pub fn insert_node(&mut self, target: &Key) {
        let d = &self.local_key.distance(target);
        let bucket_idx = d.ilog2();

        if let Some(i) = bucket_idx {
            let bucket = &mut self.kbuckets[i as usize];
            bucket.nodes.push(target.clone());
        } else {
            eprintln!("SelfEntry");
        }
    }

    pub fn closest_keys(&mut self, target: &Key) -> Vec<Key> {
        let d = self.local_key.distance(target);
        let mut closest = Vec::new();

        if let Some(mut i) = d.ilog2() {
            while closest.len() < K_VALUE {
                if i == 256 {
                    break;
                }

                let bucket = &mut self.kbuckets[i as usize];
                closest.append(&mut bucket.nodes);

                i += 1;
            }
        } else {
            eprintln!("SelfEntry");
        }

        closest.sort_by(|a, b| self.local_key.distance(a).cmp(&self.local_key.distance(b)));

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
        d.ilog2().map(|i| BucketIndex(i as usize))
    }

    fn index(&self) -> usize {
        self.0
    }
}
