use futures::channel::mpsc;
use futures::{AsyncReadExt, AsyncWriteExt, SinkExt, StreamExt};
use multiaddr::Multiaddr;
use std::collections::HashMap;
use std::error::Error;
use std::task::{Context, Poll};
use std::{io, net::SocketAddr};

use crate::key::Key;
use crate::node::KademliaEvent;
use crate::transport::{multiaddr_to_socketaddr, TcpStream};

#[derive(Debug)]
pub struct Connection {
    command_sender: mpsc::Sender<KademliaEvent>,
    remote_addr: SocketAddr,
}

#[derive(Debug)]
pub struct Pool {
    local_key: Key,
    connections: HashMap<Key, Connection>,
    pending_connections_rx: mpsc::Receiver<ConnectionEvent>,
    pending_connections_tx: mpsc::Sender<ConnectionEvent>,
}

impl Pool {
    pub fn new(local_key: Key) -> Self {
        let (pending_connections_tx, pending_connections_rx) = mpsc::channel(0);

        Self {
            local_key,
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
            self.local_key.clone(),
        ));
    }

    pub async fn add_outgoing(&mut self, stream: TcpStream, remote_addr: Multiaddr) {
        let socket_addr = multiaddr_to_socketaddr(remote_addr).unwrap();
        tokio::spawn(pending_outgoing(
            stream,
            socket_addr,
            self.pending_connections_tx.clone(),
            self.local_key.clone(),
        ));
    }

    pub fn get_connection(&mut self, key: &Key) -> Option<&mut Connection> {
        self.connections.get_mut(key)
    }

    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<PoolEvent> {
        loop {
            let event = match self.pending_connections_rx.poll_next_unpin(cx) {
                Poll::Ready(Some(ev)) => ev,
                Poll::Ready(None) => unreachable!("Got None in poll_next_unpin"),
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
                ConnectionEvent::Event { key, event } => {
                    return Poll::Ready(PoolEvent::Request { key, event });
                }
                ConnectionEvent::ConnectionFailed(e) => {
                    println!("Got error: {e}");
                }
                ConnectionEvent::Error(e) => {
                    eprintln!("Got error: {e}");
                }
            };
        }

        Poll::Pending
    }
}

impl Connection {
    pub fn send_event(&mut self, ev: KademliaEvent, cx: &mut Context<'_>) -> Option<KademliaEvent> {
        match self.poll_ready_notify_handler(cx) {
            Poll::Pending => Some(ev),
            Poll::Ready(Err(())) => None,
            Poll::Ready(Ok(())) => {
                self.command_sender
                    .try_send(ev)
                    .expect("Failed to send even on connection channel");
                None
            }
        }
    }

    pub fn poll_ready_notify_handler(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), ()>> {
        self.command_sender.poll_ready(cx).map_err(|_| ())
    }
}

async fn pending_outgoing(
    mut stream: TcpStream,
    remote_addr: SocketAddr,
    mut event_sender: mpsc::Sender<ConnectionEvent>,
    local_key: Key,
) {
    let ev = KademliaEvent::Ping { target: local_key };
    let ev = bincode::serialize(&ev).unwrap();
    stream.write_all(&ev).await.unwrap();

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
            Ok(ev) => {
                if let KademliaEvent::Ping { target } = ev {
                    event_sender
                        .send(ConnectionEvent::ConnectionEstablished {
                            key: target,
                            remote_addr,
                            stream,
                        })
                        .await
                        .unwrap();
                }
            }
        }

        break;
    }
}

async fn pending_incoming(
    mut stream: TcpStream,
    remote_addr: SocketAddr,
    mut event_sender: mpsc::Sender<ConnectionEvent>,
    local_key: Key,
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
            Ok(ev) => {
                if let KademliaEvent::Ping { target } = ev {
                    let ev = KademliaEvent::Ping { target: local_key };
                    let ev = bincode::serialize(&ev).unwrap();
                    stream.write_all(&ev).await.unwrap();

                    event_sender
                        .send(ConnectionEvent::ConnectionEstablished {
                            key: target,
                            remote_addr,
                            stream,
                        })
                        .await
                        .unwrap();
                }
            }
        }

        break;
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
                if ev.is_none() {
                    continue
                }

                let ev = bincode::serialize(&ev.unwrap()).unwrap();
                stream.write_all(&ev).await.unwrap();
            }
            bytes_read = stream.read(&mut buf) => {
                let bytes_read = match bytes_read {
                    Ok(bytes_read) => bytes_read,
                    Err(e) => {
                        event_sender
                            .send(ConnectionEvent::Error(e))
                            .await
                            .unwrap();
                        stream.close().await.unwrap();
                        break
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

                        stream.close().await.unwrap();

                        break
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

#[derive(Debug)]
pub enum PoolEvent {
    NewConnection { key: Key, remote_addr: SocketAddr },
    Request { key: Key, event: KademliaEvent },
}
