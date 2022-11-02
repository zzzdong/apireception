use std::{net::SocketAddr, time::SystemTime};

use hyper::http::{uri::Scheme, Extensions};
use hyper::Uri;

use crate::http::*;
use crate::runtime::Endpoint;

#[derive(Debug)]
pub struct GatewayContext {
    pub remote_addr: Option<SocketAddr>,
    pub start_time: SystemTime,
    pub orig_scheme: Scheme,
    pub orig_host: Option<String>,
    pub orig_uri: Uri,
    pub route_id: Option<String>,
    pub upstream_id: Option<String>,
    pub overwrite_host: bool,
    pub available_endpoints: Vec<Endpoint>,
    pub extensions: Extensions,
}

impl GatewayContext {
    pub fn new(remote_addr: Option<SocketAddr>, orig_scheme: Scheme, req: &HyperRequest) -> Self {
        GatewayContext {
            remote_addr,
            start_time: SystemTime::now(),
            orig_scheme,
            orig_host: req.uri().host().map(|h| h.to_string()),
            orig_uri: req.uri().clone(),
            route_id: None,
            upstream_id: None,
            overwrite_host: false,
            available_endpoints: Vec::new(),
            extensions: Extensions::new(),
        }
    }
}
