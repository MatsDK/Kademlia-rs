use futures::channel::mpsc;
use futures::{AsyncReadExt, AsyncWriteExt, Future, FutureExt, SinkExt, StreamExt};
use multiaddr::Multiaddr;
use std::collections::HashMap;
use std::task::{Context, Poll};
use std::{io, net::SocketAddr};

use crate::key::Key;
use crate::node::KademliaEvent;
use crate::transport::{multiaddr_to_socketaddr, TcpStream};

#[derive(Debug)]
pub struct Connection {
    remote_addr: SocketAddr,
    stream: TcpStream,
}

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

    pub async fn add_outgoing(&mut self, stream: TcpStream, remote_addr: Multiaddr) {
        let socket_addr = multiaddr_to_socketaddr(remote_addr).unwrap();
        tokio::spawn(pending_outgoing(
            stream,
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
            Poll::Pending => {}
            Poll::Ready(None) => {}
            Poll::Ready(Some(EstablishedConnectionEvent::Error(e))) => {}
            Poll::Ready(Some(EstablishedConnectionEvent::Event { key, event })) => {
                return Poll::Ready(PoolEvent::Request { key, event })
            }
        }

        loop {
            // Check if any pending connection made progress.
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
                PendingConnectionEvent::ConnectionFailed(e) => {
                    println!("Got error: {e}");
                }
                PendingConnectionEvent::Error(e) => {
                    eprintln!("Got error: {e}");
                }
            };
        }

        Poll::Pending
    }
}

impl Connection {
    pub async fn poll_read_stream(&mut self) -> Poll<Result<KademliaEvent, String>> {
        let mut buf = vec![0; 1024];

        loop {
            let bytes_read = self.stream.read(&mut buf).await;
            let bytes_read = match bytes_read {
                Ok(b) => b,
                Err(e) => {
                    eprint!("Error reading stream: {}", e);
                    return Poll::Ready(Err("Error reading stream".to_string()));
                }
            };

            if bytes_read == 0 {
                // continue
                return Poll::Pending;
            }

            return match bincode::deserialize::<KademliaEvent>(&buf[..bytes_read]) {
                Ok(event) => Poll::Ready(Ok(event)),
                Err(e) => {
                    println!("Failed to deserialize: {e}");
                    Poll::Ready(Err("failed to deserialize".to_string()))
                }
            };
        }
    }

    pub async fn send_event(&mut self, ev: &Vec<u8>) -> Result<(), ()> {
        match self.stream.write_all(ev).await {
            Err(e) => Err(()),
            Ok(_) => Ok(()),
        }
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

    pub fn poll_ready_notify_handler(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), ()>> {
        self.sender.poll_ready(cx).map_err(|_| ())
    }
}

async fn pending_outgoing(
    mut stream: TcpStream,
    remote_addr: SocketAddr,
    mut event_sender: mpsc::Sender<PendingConnectionEvent>,
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
                    .send(PendingConnectionEvent::ConnectionFailed(io::Error::new(
                        io::ErrorKind::Other,
                        "Failed to deserialize event".to_string(),
                    )))
                    .await
                    .unwrap();
            }
            Ok(ev) => {
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
        }

        break;
    }
}

async fn pending_incoming(
    mut stream: TcpStream,
    remote_addr: SocketAddr,
    mut event_sender: mpsc::Sender<PendingConnectionEvent>,
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
                    .send(PendingConnectionEvent::ConnectionFailed(io::Error::new(
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
                        .send(PendingConnectionEvent::ConnectionEstablished {
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
    mut connection: Connection,
    mut command_receiver: mpsc::Receiver<KademliaEvent>,
    mut event_sender: mpsc::Sender<EstablishedConnectionEvent>,
) {
    loop {
        tokio::select! {
            command = command_receiver.next() => {
                match command {
                    None => return,
                    Some(command) => {
                        let ev = bincode::serialize(&command).unwrap();
                        connection.send_event(&ev).await.unwrap()
                    }
                }
            }
            event = connection.poll_read_stream() => {
                match event {
                    Poll::Ready(Ok(event)) => {
                        event_sender.send(EstablishedConnectionEvent::Event { event, key: key.clone() }).await.unwrap();
                    }
                    Poll::Ready(Err(e)) => {
                        command_receiver.close();

                        event_sender.send(
                            EstablishedConnectionEvent::Error(io::Error::new(io::ErrorKind::Other, e))
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
    ConnectionFailed(io::Error),
    Error(io::Error),
}

#[derive(Debug)]
pub enum EstablishedConnectionEvent {
    Event { key: Key, event: KademliaEvent },
    Error(io::Error),
}

#[derive(Debug)]
pub enum PoolEvent {
    NewConnection { key: Key, remote_addr: SocketAddr },
    Request { key: Key, event: KademliaEvent },
}
