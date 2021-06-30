use hyper::StatusCode;

use crate::trace::TraceExecutor;

pub type HyperRequest = hyper::Request<hyper::Body>;
pub type HyperResponse = hyper::Response<hyper::Body>;
pub type HttpServer = hyper::server::conn::Http<TraceExecutor>;

pub fn not_found() -> HyperResponse {
    hyper::Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(hyper::Body::from("Not Found"))
        .unwrap()
}

pub fn upstream_all_down() -> HyperResponse {
    hyper::Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .body(hyper::Body::from("Upstream all down"))
        .unwrap()
}
