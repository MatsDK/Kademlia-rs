use futures::{stream::FusedStream, Future, FutureExt, Stream, StreamExt, TryStreamExt};
use multiaddr::{Multiaddr, Protocol};
use socket2::{Domain, Socket, Type};
use std::{
    io,
    net::{self, SocketAddr},
    pin::Pin,
    task::{Context, Poll},
    thread,
    time::Duration,
};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug)]
pub struct Transport {
    listener: TcpListenStream,
}

impl Transport {
    pub async fn new(addr: Multiaddr) -> Result<Self, String> {
        println!("{addr}");
        let socket_addr = if let Ok(sa) = multiaddr_to_socketaddr(addr.clone()) {
            sa
        } else {
            return Err("Multiaddr not supported".to_string());
        };

        let tcp_listener = TcpListener::bind(&socket_addr).await.unwrap();
        let listener = TcpListenStream {
            listener: tcp_listener,
        };

        Ok(Self { listener })
    }

    fn poll_next_event(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        loop {
            match self.listener.poll_accept(cx) {
                Poll::Pending => {}
                Poll::Ready(tcp_listener_ev) => {
                    // EXTRACT
                    match tcp_listener_ev {
                        TcpListenerEvent::Incoming {
                            stream,
                            socket_addr,
                        } => {
                            println!("Incoming stream {socket_addr}");
                        }
                        TcpListenerEvent::Error(e) => {
                            println!("Got error in tcp listener {e}");
                        }
                    }

                    return Poll::Ready(());
                }
            }

            return Poll::Pending;
        }
    }

    pub async fn dial(&self, addr: Multiaddr) -> Result<TcpStream, String> {
        let socket_addr = if let Ok(sa) = multiaddr_to_socketaddr(addr.clone()) {
            sa
        } else {
            return Err("Invalid socket address".to_string());
        };

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
impl Stream for Transport {
    type Item = ();

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.poll_next_event(cx).map(Some)
    }
}

impl FusedStream for Transport {
    fn is_terminated(&self) -> bool {
        false
    }
}

pub enum TcpListenerEvent {
    Incoming {
        stream: TcpStream,
        socket_addr: SocketAddr,
    },

    Error(io::Error),
}

#[derive(Debug)]
struct TcpListenStream {
    listener: TcpListener,
}

impl TcpListenStream {
    fn poll_accept(&self, cx: &mut Context<'_>) -> Poll<TcpListenerEvent> {
        let (stream, socket_addr) = match self.listener.poll_accept(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => return Poll::Ready(TcpListenerEvent::Error(e)),
            Poll::Ready(Ok(s)) => s,
        };

        Poll::Ready(TcpListenerEvent::Incoming {
            stream,
            socket_addr,
        })
    }
}

// impl Stream for TcpListenStream {
//     type Item = Result<TcpStream, io::Error>;

//     fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
//         match self.listener.poll_accept(cx) {
//             Poll::Ready(Ok((mut stream, socket_addr))) => {
//                 println!("got a connection: {socket_addr}");

//                 return Poll::Ready(Some(Ok(stream)));
//             }
//             Poll::Ready(Err(e)) => {
//                 println!("Error {e}");
//                 return Poll::Ready(None);
//             }
//             Poll::Pending => return Poll::Pending,
//         };
//     }
// }

// impl FusedStream for TcpListenStream {
//     fn is_terminated(&self) -> bool {
//         false
//     }
// }

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
