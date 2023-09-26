use futures::channel::mpsc;
use futures::{AsyncWriteExt, SinkExt, StreamExt};
use multiaddr::Multiaddr;
use std::collections::HashMap;
use std::task::{Context, Poll};
use std::{io, net::SocketAddr};
use tokio::sync::oneshot;

use crate::key::Key;
use crate::node::KademliaEvent;
use crate::transport::{multiaddr_to_socketaddr, Dial, TcpStream};

#[derive(Debug)]
enum Command<T> {
    Notify(T),
    Close,
}

#[derive(Debug)]
pub struct EstablishedConnection {
    endpoint: SocketAddr,
    sender: mpsc::Sender<Command<KademliaEvent>>,
}

impl EstablishedConnection {
    fn close(&self) {
        match self.sender.clone().try_send(Command::Close) {
            Ok(_) => {}
            Err(e) => eprint!("Error closing connection: {e}"),
        }
    }
}

#[derive(Debug)]
struct PendingConnection {
    peer_id: Option<Key>,
    abort_sender: Option<oneshot::Sender<()>>,
}

impl PendingConnection {
    fn abort(&mut self) {
        if let Some(abort_sender) = self.abort_sender.take() {
            drop(abort_sender);
        }
    }
}

#[derive(Debug)]
pub struct Pool {
    local_key: Key,
    connections: HashMap<Key, EstablishedConnection>,
    pending_connections: HashMap<usize, PendingConnection>,
    pending_connections_rx: mpsc::Receiver<PendingConnectionEvent>,
    pending_connections_tx: mpsc::Sender<PendingConnectionEvent>,
    established_connections_rx: mpsc::Receiver<EstablishedConnectionEvent>,
    established_connections_tx: mpsc::Sender<EstablishedConnectionEvent>,
    next_conn_id: usize,
}

impl Pool {
    pub fn new(local_key: Key) -> Self {
        let (pending_connections_tx, pending_connections_rx) = mpsc::channel(32);
        let (established_connections_tx, established_connections_rx) = mpsc::channel(32);

        Self {
            local_key,
            connections: Default::default(),
            pending_connections: Default::default(),
            pending_connections_rx,
            pending_connections_tx,
            established_connections_tx,
            established_connections_rx,
            next_conn_id: 0,
        }
    }

    fn next_conn_id(&mut self) -> usize {
        self.next_conn_id += 1;
        self.next_conn_id
    }

    pub fn add_incoming(&mut self, stream: TcpStream, remote_addr: SocketAddr) {
        let conn_id = self.next_conn_id();
        let (abort_sender, abort_receiver) = oneshot::channel();

        tokio::spawn(pending_incoming(
            conn_id,
            abort_receiver,
            stream,
            remote_addr,
            self.pending_connections_tx.clone(),
            self.local_key.clone(),
        ));

        self.pending_connections.insert(
            conn_id,
            PendingConnection {
                peer_id: None,
                abort_sender: Some(abort_sender),
            },
        );
    }

    pub fn add_outgoing(&mut self, dial: Dial, remote_addr: Multiaddr) {
        let socket_addr = multiaddr_to_socketaddr(remote_addr).unwrap();
        let conn_id = self.next_conn_id();
        let (abort_sender, abort_receiver) = oneshot::channel();

        tokio::spawn(pending_outgoing(
            dial,
            conn_id,
            abort_receiver,
            socket_addr,
            self.pending_connections_tx.clone(),
            self.local_key.clone(),
        ));

        self.pending_connections.insert(
            conn_id,
            PendingConnection {
                peer_id: None,
                abort_sender: Some(abort_sender),
            },
        );
    }

    pub fn get_connection(&mut self, key: &Key) -> Option<&mut EstablishedConnection> {
        self.connections.get_mut(key)
    }

    pub fn disconnect(&mut self, key: Key) {
        if let Some(conn) = self.connections.get(&key) {
            conn.close();
        }

        for conn in self
            .pending_connections
            .iter_mut()
            .filter_map(|(_, pending)| {
                pending
                    .peer_id
                    .map_or(false, |peer| peer == key)
                    .then_some(pending)
            })
        {
            conn.abort();
        }
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
                PendingConnectionEvent::ConnectionFailed {
                    error, remote_addr, ..
                } => return Poll::Ready(PoolEvent::ConnectionFailed { error, remote_addr }),
            };
        }

        Poll::Pending
    }
}

#[derive(Debug)]
#[allow(unused)]
pub struct Connection {
    remote_addr: SocketAddr,
    stream: TcpStream,
}

impl Connection {
    pub async fn poll_read_stream(&mut self) -> Poll<Result<KademliaEvent, String>> {
        self.stream.read_ev().await
    }

    pub async fn send_event(&mut self, ev: KademliaEvent) -> io::Result<()> {
        self.stream.write_ev(ev).await
    }

    pub async fn close(mut self) {
        self.stream.close().await.unwrap();
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
                    .try_send(Command::Notify(ev))
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
    conn_id: usize,
    abort_receiver: oneshot::Receiver<()>,
    remote_addr: SocketAddr,
    mut event_sender: mpsc::Sender<PendingConnectionEvent>,
    local_key: Key,
) {
    tokio::select! {
        abort = abort_receiver => {
            match abort {
                Err(..) => {
                    event_sender
                        .send(PendingConnectionEvent::ConnectionFailed {
                            remote_addr,
                            error: io::Error::new(io::ErrorKind::Other, format!("Connection aborted")),
                            conn_id
                        }).await.unwrap();
                }
                Ok(..) => unreachable!()
            }
        }
        stream = dial => {
            let mut stream = match stream{
                Err(error) => {
                    return event_sender
                        .send(PendingConnectionEvent::ConnectionFailed {
                            conn_id,
                            error: io::Error::new(
                                io::ErrorKind::Other,
                                format!("Failed to establish connetion: {error}"),
                            ),
                            remote_addr,
                        })
                        .await
                        .unwrap();
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
                            conn_id,
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
    }
}

async fn pending_incoming(
    conn_id: usize,
    abort_receiver: oneshot::Receiver<()>,
    mut stream: TcpStream,
    remote_addr: SocketAddr,
    mut event_sender: mpsc::Sender<PendingConnectionEvent>,
    local_key: Key,
) {
    tokio::select! {
        abort = abort_receiver => {
            match abort {
                Err(..) => {
                    event_sender
                        .send(PendingConnectionEvent::ConnectionFailed {
                            remote_addr,
                            error: io::Error::new(io::ErrorKind::Other, format!("Connection aborted")),
                            conn_id
                        }).await.unwrap();
                }
                Ok(..) => unreachable!()
            }
        }
        ev = stream.read_ev() => {
            match ev {
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
                            conn_id,
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
    }
}

async fn established_connection(
    key: Key,
    mut connection: Connection,
    mut command_receiver: mpsc::Receiver<Command<KademliaEvent>>,
    mut event_sender: mpsc::Sender<EstablishedConnectionEvent>,
) {
    loop {
        tokio::select! {
            command = command_receiver.next() => {
                match command {
                    None => return,
                    // Some(command) => connection.send_event(command).await.unwrap()
                    Some(command) => match command {
                        Command::Notify(event) => connection.send_event(event).await.unwrap(),
                        Command::Close => {
                            command_receiver.close();
                            connection.close().await;

                            event_sender.send(EstablishedConnectionEvent::Closed {
                                key,
                                error: None
                            }).await.unwrap();

                            return
                        }
                    }
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
                            EstablishedConnectionEvent::Closed { key, error: Some(error) }
                        ).await.unwrap();

                        return
                    }
                    _ => {}
                }
            }
        }
    }
}

// TODO: add separate error for inbound and outbound with custom errors
#[derive(Debug)]
pub enum PendingConnectionEvent {
    ConnectionEstablished {
        key: Key,
        remote_addr: SocketAddr,
        stream: TcpStream,
    },
    ConnectionFailed {
        conn_id: usize,
        remote_addr: SocketAddr,
        error: io::Error,
    },
}

#[derive(Debug)]
pub enum EstablishedConnectionEvent {
    Event { key: Key, event: KademliaEvent },
    Closed { key: Key, error: Option<String> },
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
        error: Option<String>,
        remote_addr: SocketAddr,
    },
    ConnectionFailed {
        error: io::Error,
        remote_addr: SocketAddr,
    },
}
