use futures::channel::mpsc;
use futures::prelude::*;
use futures::{AsyncReadExt, Future, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::BufReader;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{io, net::SocketAddr};

use crate::key::Key;
use crate::node::KademliaEvent;
use crate::transport::TcpStream;

#[derive(Debug)]
pub struct Connection {
    command_sender: mpsc::Sender<KademliaEvent>,
    remote_addr: SocketAddr,
}

#[derive(Debug)]
pub struct Pool {
    connections: HashMap<Key, Connection>,
    pending_connections_rx: mpsc::Receiver<ConnectionEvent>,
    pending_connections_tx: mpsc::Sender<ConnectionEvent>,
}

impl Pool {
    pub fn new() -> Self {
        let (pending_connections_tx, pending_connections_rx) = mpsc::channel(0);

        Self {
            connections: Default::default(),
            pending_connections_rx,
            pending_connections_tx,
        }
    }

    pub fn add_incoming(
        &mut self,
        stream: TcpStream,
        remote_addr: SocketAddr,
        // local_addr: SocketAddr,
    ) {
        tokio::spawn(pending_incoming(
            stream,
            remote_addr,
            self.pending_connections_tx.clone(),
        ));
    }

    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<ConnectionEvent> {
        loop {
            let event = match self.pending_connections_rx.poll_next_unpin(cx) {
                Poll::Ready(Some(ev)) => ev,
                Poll::Ready(None) => unreachable!("Got None in poll_next_unping"),
                Poll::Pending => break,
            };

            match event {
                ConnectionEvent::ConnectionEstablished { key, remote_addr } => {
                    let (command_sender, command_receiver) = mpsc::channel(0);
                    self.connections.insert(
                        key.clone(),
                        Connection {
                            command_sender,
                            remote_addr,
                        },
                    );
                    tokio::spawn(established_connection(
                        key.clone(),
                        command_receiver,
                        self.pending_connections_tx.clone(),
                    ));

                    return Poll::Ready(ConnectionEvent::ConnectionEstablished {
                        key,
                        remote_addr,
                    });
                }
                ConnectionEvent::ConnectionFailed(e) => {
                    println!("Got error: {e}");
                }
            };
        }

        Poll::Pending
    }
}

async fn pending_incoming(
    mut stream: TcpStream,
    remote_addr: SocketAddr,
    mut connection_tx: mpsc::Sender<ConnectionEvent>,
) {
    let mut buf = vec![0; 1024];

    loop {
        let bytes_read = stream.read(&mut buf).await.unwrap();
        if bytes_read == 0 {
            continue;
        }

        match bincode::deserialize::<KademliaEvent>(&buf[0..bytes_read]) {
            Err(e) => {
                println!("Failed to deserialize: {e}");
                connection_tx
                    .send(ConnectionEvent::ConnectionFailed(io::Error::new(
                        io::ErrorKind::Other,
                        "Failed to deserialize event".to_string(),
                    )))
                    .await
                    .unwrap();
            }
            Ok(ev) => match ev {
                KademliaEvent::Ping(key) => {
                    connection_tx
                        .send(ConnectionEvent::ConnectionEstablished { key, remote_addr })
                        .await
                        .unwrap();
                    break;
                }
            },
        }
    }
}

async fn established_connection(
    key: Key,
    mut command_receiver: mpsc::Receiver<KademliaEvent>,
    mut event_sender: mpsc::Sender<ConnectionEvent>,
) {
    loop {
        tokio::select! {
            ev = command_receiver.next() => {

            }
        }
    }
}

#[derive(Debug)]
pub enum ConnectionEvent {
    ConnectionEstablished { key: Key, remote_addr: SocketAddr },
    ConnectionFailed(io::Error),
}
