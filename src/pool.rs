use std::net::SocketAddr;
use tokio::net::TcpStream;

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
        println!("received incoming {remote_addr:?} {local_addr:?}");
    }
}
