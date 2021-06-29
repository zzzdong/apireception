use std::net::SocketAddr;

pub trait PeerAddr {
    fn peer_addr(&self) -> std::io::Result<SocketAddr>;
}

impl PeerAddr for tokio::net::TcpStream {
    fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        tokio::net::TcpStream::peer_addr(self)
    }
}

impl<T: PeerAddr> PeerAddr for tokio_rustls::client::TlsStream<T> {
    fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.get_ref().0.peer_addr()
    }
}

impl<T: PeerAddr> PeerAddr for tokio_rustls::server::TlsStream<T> {
    fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.get_ref().0.peer_addr()
    }
}