use futures::channel::mpsc;
use futures::{AsyncReadExt, SinkExt, StreamExt};
use std::collections::HashMap;
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

    pub fn add_incoming(&mut self, stream: TcpStream, remote_addr: SocketAddr) {
        tokio::spawn(pending_incoming(
            stream,
            remote_addr,
            self.pending_connections_tx.clone(),
        ));
    }

    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<PoolEvent> {
        loop {
            let event = match self.pending_connections_rx.poll_next_unpin(cx) {
                Poll::Ready(Some(ev)) => ev,
                Poll::Ready(None) => unreachable!("Got None in poll_next_unping"),
                Poll::Pending => break,
            };

            match event {
                ConnectionEvent::ConnectionEstablished {
                    key,
                    remote_addr,
                    stream,
                } => {
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
                        stream,
                        command_receiver,
                        self.pending_connections_tx.clone(),
                    ));

                    return Poll::Ready(PoolEvent::NewConnection { key, remote_addr });
                }
                ConnectionEvent::ConnectionFailed(e) => {
                    println!("Got error: {e}");
                }
                ConnectionEvent::Event { key, event } => {
                    return Poll::Ready(PoolEvent::Request { key, event });
                }
                ConnectionEvent::Error(e) => {
                    eprintln!("Got error: {e}");
                }
            };
        }

        Poll::Pending
    }
}

async fn pending_incoming(
    mut stream: TcpStream,
    remote_addr: SocketAddr,
    mut event_sender: mpsc::Sender<ConnectionEvent>,
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
                event_sender
                    .send(ConnectionEvent::ConnectionFailed(io::Error::new(
                        io::ErrorKind::Other,
                        "Failed to deserialize event".to_string(),
                    )))
                    .await
                    .unwrap();
            }
            Ok(ev) => match ev {
                KademliaEvent::Ping(key) => {
                    event_sender
                        .send(ConnectionEvent::ConnectionEstablished {
                            key,
                            remote_addr,
                            stream,
                        })
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
    mut stream: TcpStream,
    mut command_receiver: mpsc::Receiver<KademliaEvent>,
    mut event_sender: mpsc::Sender<ConnectionEvent>,
) {
    let mut buf = vec![0; 1024];

    loop {
        tokio::select! {
            ev = command_receiver.next() => {

            }
            bytes_read = stream.read(&mut buf) => {
                let bytes_read = match bytes_read {
                    Ok(bytes_read) => bytes_read,
                    Err(e) => {
                        event_sender
                            .send(ConnectionEvent::Error(e))
                            .await
                            .unwrap();
                        continue
                    }
                };

                if bytes_read == 0 {
                    continue
                }

                match bincode::deserialize::<KademliaEvent>(&buf[..bytes_read]) {
                    Ok(event) => {
                        event_sender.send(ConnectionEvent::Event { event, key: key.clone() }).await.unwrap();
                    }
                    Err(e) => {
                        println!("Failed to deserialize: {e}");
                        event_sender
                            .send(ConnectionEvent::Error(io::Error::new(
                                        io::ErrorKind::Other,
                                        "Failed to deserialize event".to_string(),
                                        )))
                            .await
                            .unwrap();
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum ConnectionEvent {
    ConnectionEstablished {
        key: Key,
        remote_addr: SocketAddr,
        stream: TcpStream,
    },
    ConnectionFailed(io::Error),
    Event {
        key: Key,
        event: KademliaEvent,
    },
    Error(io::Error),
}

pub enum PoolEvent {
    NewConnection { key: Key, remote_addr: SocketAddr },
    Request { key: Key, event: KademliaEvent },
}
