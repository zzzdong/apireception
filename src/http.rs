use std::{net::SocketAddr, pin::Pin};

use futures::Future;
use headers::HeaderValue;
use hyper::StatusCode;

const X_FORWARDED_FOR: &str = "x-forwarded-for";
const X_REAL_IP: &str = "x-real-ip";

pub type HyperRequest = hyper::Request<hyper::Body>;
pub type HyperResponse = hyper::Response<hyper::Body>;
pub type HttpServer = hyper::server::conn::Http<crate::trace::TraceExecutor>;
pub type ResponseFuture =
    Pin<Box<dyn Future<Output = Result<HyperResponse, crate::Error>> + Send + 'static>>;

#[derive(Debug, Clone)]
pub struct RemoteInfo {
    pub addr: SocketAddr,
}

impl RemoteInfo {
    pub fn new(addr: SocketAddr) -> Self {
        RemoteInfo { addr }
    }
}

pub fn set_proxy_headers(req: &mut HyperRequest, info: &RemoteInfo) {
    let x_forwarded_for = req.headers().get(X_FORWARDED_FOR);

    let x_forwarded_for = match x_forwarded_for {
        Some(exist_forwarded_for) => {
            let mut forwarded_for = exist_forwarded_for.to_str().unwrap_or("").to_string();
            forwarded_for.push_str(&format!(", {}", info.addr.to_string()));
            forwarded_for
        }
        None => info.addr.to_string(),
    };

    req.headers_mut().insert(
        X_FORWARDED_FOR,
        HeaderValue::from_str(&x_forwarded_for).expect("HeaderValue failed"),
    );

    req.headers_mut().insert(
        X_REAL_IP,
        HeaderValue::from_str(&info.addr.ip().to_string()).expect("HeaderValue failed"),
    );
}

pub fn not_found() -> HyperResponse {
    hyper::Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(hyper::Body::from("Not Found"))
        .unwrap()
}

pub fn upstream_unavailable() -> HyperResponse {
    hyper::Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .body(hyper::Body::from("Upstream Unavailable"))
        .unwrap()
}

pub fn bad_gateway() -> HyperResponse {
    hyper::Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .body(hyper::Body::from("Bad Gateway"))
        .unwrap()
}
