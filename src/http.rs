use std::pin::Pin;

use futures::Future;
use hyper::StatusCode;

pub const X_FORWARDED_FOR: &str = "x-forwarded-for";
pub const X_FORWARDED_HOST: &str = "x-forwarded-host";
pub const X_FORWARDED_PROTO: &str = "x-forwarded-proto";
pub const X_REAL_IP: &str = "x-real-ip";

pub type HyperRequest = hyper::Request<hyper::Body>;
pub type HyperResponse = hyper::Response<hyper::Body>;
pub type HttpServer = hyper::server::conn::Http<crate::trace::TraceExecutor>;
pub type ResponseFuture =
    Pin<Box<dyn Future<Output = Result<HyperResponse, crate::Error>> + Send + 'static>>;

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
