use futures::Future;
use serde::{Deserialize, Serialize};
use std::{io, net::SocketAddr};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::key::Key;
use crate::node::KademliaEvent;

#[derive(Debug)]
pub struct Pool {}

impl Pool {
    pub fn new() -> Self {
        Self {}
    }

    pub fn add_incoming(
        &mut self,
        stream: TcpStream,
        remote_addr: SocketAddr,
        local_addr: SocketAddr,
    ) {
        tokio::spawn(pending_incoming(stream, remote_addr));
    }
}

async fn pending_incoming(mut stream: TcpStream, remote_addr: SocketAddr) {
    let mut buf = vec![0; 1024];
    let (mut reader, _writer) = stream.split();

    loop {
        match reader.read(&mut buf).await {
            Ok(b) => {
                if b == 0 {
                    continue;
                }
                match bincode::deserialize::<KademliaEvent>(&buf[0..b]) {
                    Err(e) => {
                        println!("Failed to deserialize: {e}");
                    }
                    Ok(ev) => match ev {
                        KademliaEvent::Ping(key) => {
                            println!("Obtained key {key:?} {}", remote_addr);
                            break;
                        }
                    },
                }
            }
            Err(e) => {
                println!("Got error reading bytes {e}")
            }
        }
    }
}
