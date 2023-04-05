use futures::channel::mpsc;
use futures::{SinkExt, StreamExt};
use multiaddr::Multiaddr;
use std::collections::HashMap;
use std::task::{Context, Poll};
use std::{io, net::SocketAddr};

use crate::key::Key;
use crate::node::KademliaEvent;
use crate::transport::{multiaddr_to_socketaddr, Dial, TcpStream};

#[derive(Debug)]
pub struct EstablishedConnection {
    endpoint: SocketAddr,
    sender: mpsc::Sender<KademliaEvent>,
}

#[derive(Debug)]
pub struct Pool {
    local_key: Key,
    connections: HashMap<Key, EstablishedConnection>,
    pending_connections_rx: mpsc::Receiver<PendingConnectionEvent>,
    pending_connections_tx: mpsc::Sender<PendingConnectionEvent>,
    established_connections_rx: mpsc::Receiver<EstablishedConnectionEvent>,
    established_connections_tx: mpsc::Sender<EstablishedConnectionEvent>,
}

impl Pool {
    pub fn new(local_key: Key) -> Self {
        let (pending_connections_tx, pending_connections_rx) = mpsc::channel(32);
        let (established_connections_tx, established_connections_rx) = mpsc::channel(32);

        Self {
            local_key,
            connections: Default::default(),
            pending_connections_rx,
            pending_connections_tx,
            established_connections_tx,
            established_connections_rx,
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

    pub fn add_outgoing(&mut self, dial: Dial, remote_addr: Multiaddr) {
        let socket_addr = multiaddr_to_socketaddr(remote_addr).unwrap();
        tokio::spawn(pending_outgoing(
            dial,
            socket_addr,
            self.pending_connections_tx.clone(),
            self.local_key.clone(),
        ));
    }

    pub fn get_connection(&mut self, key: &Key) -> Option<&mut EstablishedConnection> {
        self.connections.get_mut(key)
    }

    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<PoolEvent> {
        // Prioritize established connections.
        match self.established_connections_rx.poll_next_unpin(cx) {
            Poll::Ready(Some(EstablishedConnectionEvent::Closed { key, error })) => {
                let closed_conn = self
                    .connections
                    .remove(&key)
                    .expect("Connection is established");

                return Poll::Ready(PoolEvent::ConnectionClosed {
                    key,
                    error,
                    remote_addr: closed_conn.endpoint,
                });
            }
            Poll::Ready(Some(EstablishedConnectionEvent::Event { key, event })) => {
                return Poll::Ready(PoolEvent::Request { key, event })
            }
            Poll::Pending => {}
            Poll::Ready(None) => {}
        }

        loop {
            // Check if a pending connection made progress.
            let event = match self.pending_connections_rx.poll_next_unpin(cx) {
                Poll::Ready(Some(ev)) => ev,
                Poll::Ready(None) => unreachable!("Got None in poll_next_unpin"),
                Poll::Pending => break,
            };

            match event {
                PendingConnectionEvent::ConnectionEstablished {
                    key,
                    remote_addr,
                    stream,
                } => {
                    let (command_sender, command_receiver) = mpsc::channel(32);
                    let connection = Connection {
                        remote_addr,
                        stream,
                    };
                    self.connections.insert(
                        key.clone(),
                        EstablishedConnection {
                            sender: command_sender,
                            endpoint: remote_addr,
                        },
                    );

                    // Initiate a new thread for handeling an established peer connection
                    tokio::spawn(established_connection(
                        key.clone(),
                        connection,
                        command_receiver,
                        self.established_connections_tx.clone(),
                    ));

                    return Poll::Ready(PoolEvent::NewConnection { key, remote_addr });
                }
                PendingConnectionEvent::ConnectionFailed { error, remote_addr } => {
                    return Poll::Ready(PoolEvent::ConnectionFailed { error, remote_addr })
                }
            };
        }

        Poll::Pending
    }
}

#[derive(Debug)]
pub struct Connection {
    remote_addr: SocketAddr,
    stream: TcpStream,
}

impl Connection {
    pub async fn poll_read_stream(&mut self) -> Poll<Result<KademliaEvent, String>> {
        self.stream.read_ev().await
    }

    pub async fn send_event(&mut self, ev: KademliaEvent) -> Result<(), io::Error> {
        self.stream.write_ev(ev).await
    }
}

impl EstablishedConnection {
    pub fn send_event(&mut self, ev: KademliaEvent, cx: &mut Context<'_>) -> Option<KademliaEvent> {
        // Before notifying a connection thread, `poll_ready_notify_handler` checks if the
        // `command_receiver` is ready to receive an event, if not, return the event to try again later.
        match self.poll_ready_notify_handler(cx) {
            Poll::Pending => Some(ev),
            Poll::Ready(Err(())) => None,
            Poll::Ready(Ok(())) => {
                self.sender
                    .try_send(ev)
                    .expect("failed to send to receiver for connection");
                None
            }
        }
    }

    // This will return `None` if the sender does not have enough capacity to receive a message.
    pub fn poll_ready_notify_handler(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), ()>> {
        self.sender.poll_ready(cx).map_err(|_| ())
    }
}

async fn pending_outgoing(
    dial: Dial,
    remote_addr: SocketAddr,
    mut event_sender: mpsc::Sender<PendingConnectionEvent>,
    local_key: Key,
) {
    let mut stream = match dial.await {
        Err(e) => {
            eprintln!("Error occured while connection to {remote_addr} in dial: {e}");
            return;
        }
        Ok(s) => s,
    };

    let ev = KademliaEvent::Ping { target: local_key };
    stream.write_ev(ev).await.unwrap();

    match stream.read_ev().await {
        Poll::Ready(Ok(ev)) => {
            if let KademliaEvent::Ping { target } = ev {
                event_sender
                    .send(PendingConnectionEvent::ConnectionEstablished {
                        key: target,
                        remote_addr,
                        stream,
                    })
                    .await
                    .unwrap();
            }
        }
        Poll::Ready(Err(error)) => {
            event_sender
                .send(PendingConnectionEvent::ConnectionFailed {
                    error: io::Error::new(
                        io::ErrorKind::Other,
                        format!("Failed to establish connetion: {error}"),
                    ),
                    remote_addr,
                })
                .await
                .unwrap();
        }
        _ => {}
    }
}

async fn pending_incoming(
    mut stream: TcpStream,
    remote_addr: SocketAddr,
    mut event_sender: mpsc::Sender<PendingConnectionEvent>,
    local_key: Key,
) {
    match stream.read_ev().await {
        Poll::Ready(Ok(ev)) => {
            if let KademliaEvent::Ping { target } = ev {
                let ev = KademliaEvent::Ping { target: local_key };
                stream.write_ev(ev).await.unwrap();

                event_sender
                    .send(PendingConnectionEvent::ConnectionEstablished {
                        key: target,
                        remote_addr,
                        stream,
                    })
                    .await
                    .unwrap();
            } else {
                eprintln!("Received wrong event in pending_incoming");
            }
        }
        Poll::Ready(Err(error)) => {
            event_sender
                .send(PendingConnectionEvent::ConnectionFailed {
                    error: io::Error::new(
                        io::ErrorKind::Other,
                        format!("Failed to establish connection: {error}"),
                    ),
                    remote_addr,
                })
                .await
                .unwrap();
        }
        _ => {}
    }
}

async fn established_connection(
    key: Key,
    mut connection: Connection,
    mut command_receiver: mpsc::Receiver<KademliaEvent>,
    mut event_sender: mpsc::Sender<EstablishedConnectionEvent>,
) {
    loop {
        tokio::select! {
            command = command_receiver.next() => {
                match command {
                    None => return,
                    Some(command) => connection.send_event(command).await.unwrap()
                }
            }
            event = connection.poll_read_stream() => {
                match event {
                    Poll::Ready(Ok(event)) => {
                        event_sender.send(
                            EstablishedConnectionEvent::Event { event, key: key.clone() }
                        ).await.unwrap();
                    }
                    Poll::Ready(Err(error)) => {
                        command_receiver.close();

                        event_sender.send(
                            EstablishedConnectionEvent::Closed { key, error }
                        ).await.unwrap();

                        return
                    }
                    _ => {}
                }

            }
        }
    }
}

#[derive(Debug)]
pub enum PendingConnectionEvent {
    ConnectionEstablished {
        key: Key,
        remote_addr: SocketAddr,
        stream: TcpStream,
    },
    ConnectionFailed {
        remote_addr: SocketAddr,
        error: io::Error,
    },
}

#[derive(Debug)]
pub enum EstablishedConnectionEvent {
    Event { key: Key, event: KademliaEvent },
    Closed { key: Key, error: String },
}

#[derive(Debug)]
pub enum PoolEvent {
    NewConnection {
        key: Key,
        remote_addr: SocketAddr,
    },
    Request {
        key: Key,
        event: KademliaEvent,
    },
    ConnectionClosed {
        key: Key,
        error: String,
        remote_addr: SocketAddr,
    },
    ConnectionFailed {
        error: io::Error,
        remote_addr: SocketAddr,
    },
}
