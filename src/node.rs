use futures::StreamExt;
use multiaddr::Multiaddr;
use std::io;

use crate::{key::Key, transport::Transport, K_VALUE};

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

#[derive(Debug)]
pub struct KademliaNode {
    routing_table: RoutingTable,
}

impl KademliaNode {
    pub async fn new(key: Key, addr: Multiaddr) -> io::Result<Self> {
        let mut transport = Transport::default();
        transport.listen_on(addr).await.unwrap();

        Ok(Self {
            routing_table: RoutingTable::new(key),
        })
    }

    pub fn local_key(&self) -> &Key {
        &self.routing_table.local_key
    }

    pub fn add_address(&mut self, target: &Key) {
        self.routing_table.insert_node(target);
    }

    pub fn find_nodes(&mut self, target: &Key) -> Vec<Key> {
        self.routing_table.closest_keys(target)
    }
}
