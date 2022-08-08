use std::fmt::Write;
use std::{net::SocketAddr, time::SystemTime};

use headers::HeaderValue;
use hyper::http::{uri::Scheme, Extensions};

use crate::{http::*, services::AppendInfo};

pub struct GatewayInfo {
    pub request_info: RequestInfo,
    pub upstream_id: Option<String>,
    pub extensions: Extensions,
}

#[derive(Debug, Clone)]
pub struct RequestInfo {
    pub remote_addr: SocketAddr,
    pub start_time: SystemTime,
    pub scheme: Scheme,
    pub host: Option<String>,
}

impl RequestInfo {
    pub fn new(scheme: Scheme, remote_addr: SocketAddr) -> Self {
        RequestInfo {
            remote_addr,
            scheme,
            host: None,
            start_time: SystemTime::now(),
        }
    }

    pub fn append_proxy_headers(&self, req: &mut HyperRequest) {
        let x_forwarded_for = req.headers().get(X_FORWARDED_FOR);

        let x_forwarded_for = match x_forwarded_for {
            Some(exist_forwarded_for) => {
                let mut forwarded_for = exist_forwarded_for.to_str().unwrap_or("").to_string();
                write!(forwarded_for, ", {}", self.remote_addr).unwrap();
                forwarded_for
            }
            None => self.remote_addr.to_string(),
        };

        req.headers_mut().insert(
            X_FORWARDED_FOR,
            HeaderValue::from_str(&x_forwarded_for).expect("HeaderValue failed"),
        );

        req.headers_mut().insert(
            X_REAL_IP,
            HeaderValue::from_str(&self.remote_addr.ip().to_string()).expect("HeaderValue failed"),
        );

        req.headers_mut().insert(
            X_FORWARDED_PROTO,
            HeaderValue::from_str(self.scheme.as_str()).expect("HeaderValue failed"),
        );

        if let Some(ref host) = self.host {
            req.headers_mut().insert(
                X_FORWARDED_HOST,
                HeaderValue::from_str(host).expect("HeaderValue failed"),
            );
        }
    }
}

impl AppendInfo for RequestInfo {
    fn append_info(&mut self, req: HyperRequest) {
        self.host = req.uri().host().map(|h| h.to_string());
    }
}
