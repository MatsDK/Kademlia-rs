use futures::{stream::FusedStream, Stream, StreamExt};
use multiaddr::{Multiaddr, Protocol};
use socket2::{Domain, Socket, Type};
use std::{
    io,
    net::{self, SocketAddr},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug)]
pub struct Transport {
    // listener: Option<Pin<Box<TcpListenStream>>>
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            // listener: None
        }
    }
}

impl Transport {
    pub async fn listen_on(&mut self, addr: Multiaddr) -> Result<(), String> {
        let socket_addr = if let Ok(sa) = multiaddr_to_socketaddr(addr.clone()) {
            sa
        } else {
            return Err("Multiaddr not supported".to_string());
        };

        let listener = TcpListener::bind(&socket_addr).await.unwrap();

        let listen_stream = TcpListenStream { listener };
        self.spawn_listener(listen_stream);

        println!("Listen on {addr}");
        println!("Listen on {socket_addr}");
        // println!("Listen on local {local_addr}");

        // loop {}

        Ok(())
    }

    fn spawn_listener(&self, mut listener: TcpListenStream) {
        tokio::spawn(async move {
            while let Ok(stream) = listener.select_next_some().await {
                // let test  = future::ok::<TcpStream, io::Error>(stream);
                // match Box::pin(test).await {
                //     Ok(s) => {

                //         s

                //     }
                //     Err(e) => {
                //         eprintln!("{e}")
                //     }

                // }
                println!("got incoming stream")
            }
        });
    }

    // type Output = ();
    // type Error = io::Error;
    // type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    pub async fn dial(&self, addr: Multiaddr) -> Result<TcpStream, String> {
        let socket_addr = if let Ok(sa) = multiaddr_to_socketaddr(addr.clone()) {
            sa
        } else {
            return Err("Invalid socket address".to_string());
        };

        println!("{socket_addr}");
        let socket = self.create_socket(&socket_addr).unwrap();
        // socket.set_nonblocking(true).unwrap();

        match socket.connect(&socket_addr.into()) {
            Ok(()) => {}
            Err(err) => return Err(err.to_string()),
        }

        let s: net::TcpStream = socket.into();
        let stream = tokio::net::TcpStream::try_from(s).unwrap();

        stream.writable().await.unwrap();

        if let Some(e) = stream.take_error().unwrap() {
            return Err(e.to_string());
        }

        Ok(stream)
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

#[derive(Debug)]
struct TcpListenStream {
    listener: TcpListener,
}

impl Stream for TcpListenStream {
    type Item = Result<TcpStream, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.listener.poll_accept(cx) {
            Poll::Ready(Ok((mut stream, socket_addr))) => {
                // println!("got a connection: {socket_addr}");

                return Poll::Ready(Some(Ok(stream)));
            }
            Poll::Ready(Err(e)) => {
                println!("Error {e}");
                return Poll::Ready(None);
            }
            Poll::Pending => return Poll::Pending,
        };
    }
}

impl FusedStream for TcpListenStream {
    fn is_terminated(&self) -> bool {
        false
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
