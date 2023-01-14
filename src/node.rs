use futures::{stream::FusedStream, Stream};
use multiaddr::Multiaddr;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    io,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    key::Key,
    pool::{Pool, PoolEvent},
    query::{Query, QueryPool, QueryPoolState},
    routing::RoutingTable,
    transport::{socketaddr_to_multiaddr, Transport, TransportEvent},
};

#[derive(Debug)]
pub struct KademliaNode {
    routing_table: RoutingTable,
    transport: Transport,
    pool: Pool,
    // queued_queries: VecDeque<Query>,
    queries: QueryPool,
    connected_peers: HashSet<Key>,
}

impl KademliaNode {
    pub async fn new(key: Key, addr: impl Into<Multiaddr>) -> io::Result<Self> {
        let addr = addr.into();
        println!(">> Listening {addr} >> {key}");
        let transport = Transport::new(&addr).await.unwrap();

        Ok(Self {
            routing_table: RoutingTable::new(key.clone()),
            transport,
            pool: Pool::new(key),
            // queued_queries: VecDeque::new(),
            queries: QueryPool::new(),
            connected_peers: HashSet::new(),
        })
    }

    pub async fn dial(&mut self, addr: impl Into<Multiaddr>) -> io::Result<()> {
        let addr = addr.into();
        let stream = self.transport.dial(&addr).await.unwrap();

        self.pool.add_outgoing(stream, addr).await;

        Ok(())
    }

    pub fn local_key(&self) -> &Key {
        &self.routing_table.local_key
    }

    pub fn add_address(&mut self, key: &Key, addr: Multiaddr) {
        self.routing_table.insert(key, addr);
    }

    pub fn find_node(&mut self, target: &Key) {
        let peers = self.routing_table.closest_nodes(target);
        // let query = Query::new();
        // self.queued_queries.push_back(query);
        self.queries.add_query(
            peers,
            KademliaEvent::FindNode {
                target: target.clone(),
            },
        );
    }

    fn handle_transport_event(&mut self, ev: TransportEvent) {
        match ev {
            TransportEvent::Incoming {
                stream,
                socket_addr,
            } => {
                self.pool.add_incoming(stream, socket_addr);
            }
            TransportEvent::Error(e) => {
                println!("Got error {e}");
            }
        }
    }

    fn handle_pool_event(&mut self, ev: PoolEvent) {
        match ev {
            PoolEvent::NewConnection { key, remote_addr } => {
                println!("Established connection {key} {remote_addr}");
                let endpoint = socketaddr_to_multiaddr(&remote_addr);
                self.add_address(&key, endpoint);
                self.connected_peers.insert(key);
            }
            PoolEvent::Request { key, event } => {
                println!("new request from {key}: {event:?}")
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

            match self.pool.poll(cx) {
                Poll::Pending => {}
                Poll::Ready(connection_ev) => {
                    self.handle_pool_event(connection_ev);
                    return Poll::Ready(());
                }
            }

            loop {
                match self.queries.poll() {
                    QueryPoolState::Waiting((q, key)) => {
                        if self.connected_peers.contains(&key) {
                            println!("should send event to {key} {q:?}");
                        } else {
                            println!("should connect to peer {key}");
                        }
                    }
                    QueryPoolState::Finished(q) => {
                        println!("Query finished: {:?}", q);
                        break;
                    }
                    QueryPoolState::Idle => break,
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum KademliaEvent {
    Ping(Key),
    FindNode { target: Key },
}
