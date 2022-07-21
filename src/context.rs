use std::net::SocketAddr;

use hyper::http::Extensions;

pub struct GatewayContext {
    pub remote_addr: SocketAddr,
    pub upstream_id: String,
    pub extensions: Extensions,
}
