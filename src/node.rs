use futures::{stream::FusedStream, Stream, StreamExt};
use multiaddr::Multiaddr;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
    thread,
    time::Duration,
};

use crate::{
    key::Key,
    pool::Pool,
    transport::{Transport, TransportEvent},
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
    transport: Transport,
    pool: Pool,
}

impl KademliaNode {
    pub async fn new(key: Key, addr: impl Into<Multiaddr>) -> io::Result<Self> {
        let addr = addr.into();
        let transport = Transport::new(addr.clone()).await.unwrap();

        Ok(Self {
            routing_table: RoutingTable::new(key),
            transport,
            pool: Pool::new(),
        })
    }

    pub async fn dial(&self, addr: impl Into<Multiaddr>) -> io::Result<()> {
        let addr = addr.into();
        self.transport.dial(addr).await.unwrap();

        Ok(())
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

    fn handle_transport_event(&mut self, ev: TransportEvent) {
        match ev {
            TransportEvent::Incoming {
                stream,
                socket_addr,
            } => {
                let local_addr = stream.local_addr().unwrap();
                self.pool.add_incoming(stream, socket_addr, local_addr);
            }
            TransportEvent::Error(e) => {
                println!("Got error {e}");
            }
        }
    }

    fn poll_next_event(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        loop {
            match self.transport.poll(cx) {
                Poll::Pending => {}
                Poll::Ready(transport_ev) => {
                    self.handle_transport_event(transport_ev);
                    return Poll::Ready(());
                }
            }

            return Poll::Pending;
        }
    }
}

impl Stream for KademliaNode {
    type Item = ();

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.poll_next_event(cx).map(Some)
    }
}

impl FusedStream for KademliaNode {
    fn is_terminated(&self) -> bool {
        false
    }
}
