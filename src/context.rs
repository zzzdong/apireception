use std::net::SocketAddr;

pub struct GatewayContext {
    pub remote_addr: SocketAddr,
    pub upstream_id: String,
}
