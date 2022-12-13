use multiaddr::{Multiaddr, Protocol};
use socket2::{Domain, Socket, Type};
use std::{
    error::Error,
    io,
    net::{SocketAddr, TcpListener},
};

pub struct Transport {}

impl Transport {
    pub fn listen_on(&self, addr: Multiaddr) -> Result<(), String> {
        let socket_addr = if let Ok(sa) = multiaddr_to_socketaddr(addr.clone()) {
            sa
        } else {
            return Err("Multiaddr not supported".to_string());
        };

        let socket = self.create_socket(&socket_addr).unwrap();
        socket.bind(&socket_addr.into()).unwrap();
        socket.set_nonblocking(true).unwrap();

        let listener: TcpListener = socket.into();
        let local_addr = listener.local_addr().unwrap();

        println!("Listen on {addr}");
        println!("Listen on local {local_addr}");
        Ok(())
    }

    fn create_socket(&self, socket_addr: &SocketAddr) -> io::Result<Socket> {
        let domain = if socket_addr.is_ipv4() {
            Domain::IPV4
        } else {
            Domain::IPV6
        };

        let socket = Socket::new(domain, Type::STREAM, Some(socket2::Protocol::TCP))?;
        if socket_addr.is_ipv6() {
            socket.set_only_v6(true)?;
        }
        socket.set_nodelay(true)?;

        Ok(socket)
    }
}

fn multiaddr_to_socketaddr(mut addr: Multiaddr) -> Result<SocketAddr, ()> {
    // "Pop" the IP address and TCP port from the end of the address,
    // ignoring a `/p2p/...` suffix as well as any prefix of possibly
    // outer protocols, if present.
    let mut port = None;
    while let Some(proto) = addr.pop() {
        match proto {
            Protocol::Ip4(ipv4) => match port {
                Some(port) => return Ok(SocketAddr::new(ipv4.into(), port)),
                None => return Err(()),
            },
            Protocol::Ip6(ipv6) => match port {
                Some(port) => return Ok(SocketAddr::new(ipv6.into(), port)),
                None => return Err(()),
            },
            Protocol::Tcp(portnum) => match port {
                Some(_) => return Err(()),
                None => port = Some(portnum),
            },
            Protocol::P2p(_) => {}
            _ => return Err(()),
        }
    }
    Err(())
}
