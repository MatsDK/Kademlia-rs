use crate::{key::Key, K_VALUE};

#[derive(Debug, Clone)]
struct KBucket {
    nodes: Vec<Key>,
}

impl KBucket {
    fn new() -> Self {
        Self { nodes: Vec::new() }
    }
}

#[derive(Clone, Debug)]
struct RoutingTable {
    local_key: Key,
    kbuckets: Vec<KBucket>,
}

impl RoutingTable {
    fn new(key: Key) -> Self {
        Self {
            kbuckets: (0..256).map(|_| KBucket::new()).collect(),
            local_key: key,
        }
    }

    fn insert_node(&mut self, target: &Key) {
        let d = &self.local_key.distance(target);
        let bucket_idx = d.diff_bits();

        if let Some(i) = bucket_idx {
            let bucket = &mut self.kbuckets[i as usize];
            bucket.nodes.push(target.clone());
        } else {
            eprintln!("SelfEntry");
        }
    }

    fn closest_keys(&mut self, target: &Key) -> Vec<Key> {
        let d = self.local_key.distance(target);
        let mut closest = Vec::new();

        if let Some(mut i) = d.diff_bits() {
            while closest.len() < K_VALUE {
                if i == 256 {
                    break;
                }

                println!("{i}");
                let bucket = &mut self.kbuckets[i as usize];
                closest.append(&mut bucket.nodes);

                i += 1;
            }
        } else {
            eprintln!("SelfEntry");
        }

        closest.sort_by(|a, b| self.local_key.distance(a).cmp(&self.local_key.distance(b)));

        closest[..K_VALUE].to_vec()
    }
}

#[derive(Clone, Debug)]
pub struct KademliaNode {
    routing_table: RoutingTable,
}

impl KademliaNode {
    pub fn new(key: Key) -> Self {
        Self {
            routing_table: RoutingTable::new(key),
        }
    }

    pub fn local_key(&self) -> &Key {
        &self.routing_table.local_key
    }

    pub fn add_address(&mut self, target: &Key) {
        self.routing_table.insert_node(target);
    }

    pub fn find_nodes(&mut self, target: &Key) {
        println!("{:?}", self.routing_table.closest_keys(target));
    }
}
