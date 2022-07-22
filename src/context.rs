use std::net::SocketAddr;

use hyper::http::Extensions;

pub struct GatewayInfo {
    pub request_info: SocketAddr,
    pub upstream_id: Option<String>,
    pub extensions: Extensions,
}
