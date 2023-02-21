use futures::prelude::*;
use multiaddr::{Multiaddr, Protocol};
use socket2::{Domain, Socket, Type};
use std::{
    io,
    net::{self, SocketAddr},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::net::TcpListener;

#[derive(Debug)]
pub struct Transport {
    listener: TcpListenStream,
}

// TODO: refactor error handling, remove unwraps
impl Transport {
    pub async fn new(addr: &Multiaddr) -> Result<Self, String> {
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

    pub async fn dial(&self, addr: &Multiaddr) -> Result<TcpStream, String> {
        let socket_addr = if let Ok(sa) = multiaddr_to_socketaddr(addr.clone()) {
            sa
        } else {
            return Err("Invalid socket address".to_string());
        };

        let socket = self.create_socket(&socket_addr).unwrap();

        // Set socket to nonblocking mode, this way the individual connnection
        // threads will not block and prevent themselves from receiving commands.
        socket.set_nonblocking(true).unwrap();

        //     let dial_fut = Ok(async move {
        //         match socket.connect(&socket_addr.into()) {
        //             Ok(()) => {}
        //             Err(err) if err.raw_os_error() == Some(libc::EINPROGRESS) => {}
        //             Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
        //             Err(err) => return Err(err),
        //         }

        //         let s: std::net::TcpStream = socket.into();
        //         let stream = tokio::net::TcpStream::try_from(s).unwrap();

        //         stream.writable().await.unwrap();

        //         if let Some(e) = stream.take_error()? {
        //             return Err(e);
        //         }

        //         Ok(TcpStream(stream))
        //     }.boxed());

        match socket.connect(&socket_addr.into()) {
            Ok(()) => {}
            Err(err) if err.raw_os_error() == Some(libc::EINPROGRESS) => {}
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
            Err(err) => return Err(err.to_string()),
        }

        let s: std::net::TcpStream = socket.into();
        let stream = tokio::net::TcpStream::try_from(s).unwrap();

        stream.writable().await.unwrap();

        if let Some(e) = stream.take_error().unwrap() {
            return Err(e.to_string());
        }

        Ok(TcpStream(stream))
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
        // socket.set_nonblocking(true).unwrap();

        Ok(socket)
    }

    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<TransportEvent> {
        loop {
            match self.listener.poll_accept(cx) {
                Poll::Pending => {}
                Poll::Ready(tcp_listener_ev) => {
                    return Poll::Ready(tcp_listener_ev);
                }
            }

            return Poll::Pending;
        }
    }
}

#[derive(Debug)]
pub struct TcpStream(pub tokio::net::TcpStream);

impl From<TcpStream> for tokio::net::TcpStream {
    fn from(t: TcpStream) -> tokio::net::TcpStream {
        t.0
    }
}

impl AsyncRead for TcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        let mut read_buf = tokio::io::ReadBuf::new(buf);
        futures::ready!(tokio::io::AsyncRead::poll_read(
            Pin::new(&mut self.0),
            cx,
            &mut read_buf
        ))?;
        Poll::Ready(Ok(read_buf.filled().len()))
    }
}

impl AsyncWrite for TcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        tokio::io::AsyncWrite::poll_write(Pin::new(&mut self.0), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        tokio::io::AsyncWrite::poll_flush(Pin::new(&mut self.0), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        tokio::io::AsyncWrite::poll_shutdown(Pin::new(&mut self.0), cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        tokio::io::AsyncWrite::poll_write_vectored(Pin::new(&mut self.0), cx, bufs)
    }
}

pub enum TransportEvent {
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
    fn poll_accept(&self, cx: &mut Context<'_>) -> Poll<TransportEvent> {
        let (stream, socket_addr) = match self.listener.poll_accept(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => return Poll::Ready(TransportEvent::Error(e)),
            Poll::Ready(Ok(s)) => s,
        };

        Poll::Ready(TransportEvent::Incoming {
            stream: TcpStream(stream),
            socket_addr,
        })
    }
}

pub fn multiaddr_to_socketaddr(mut addr: Multiaddr) -> Result<SocketAddr, ()> {
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

/// Turns an IP address and port into the corresponding QUIC multiaddr.
pub fn socketaddr_to_multiaddr(socket_addr: &SocketAddr) -> Multiaddr {
    Multiaddr::empty()
        .with(socket_addr.ip().into())
        .with(Protocol::Tcp(socket_addr.port()))
}
