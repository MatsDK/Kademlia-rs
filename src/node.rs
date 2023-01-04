use futures::{stream::FusedStream, Stream};
use multiaddr::Multiaddr;
use serde::{Deserialize, Serialize};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    key::Key,
    pool::{Pool, PoolEvent},
    routing::RoutingTable,
    transport::{Transport, TransportEvent},
};

#[derive(Debug)]
pub struct KademliaNode {
    routing_table: RoutingTable,
    transport: Transport,
    pool: Pool,
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
                self.add_address(&key);
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

#[derive(Serialize, Deserialize, Debug)]
pub enum KademliaEvent {
    Ping(Key),
}
