use crate::key::Key;

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

    fn insert_node(&mut self, target: Key) {
        let d = &self.local_key.distance(&target);
        let bucket_idx = d.diff_bits();

        if let Some(i) = bucket_idx {
            let bucket = &mut self.kbuckets[i as usize];
            bucket.nodes.push(target);
        } else {
            eprintln!("SelfEntry");
        }
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

    pub fn add_address(&mut self, target: KademliaNode) {
        self.routing_table
            .insert_node(target.routing_table.local_key);
    }
}
