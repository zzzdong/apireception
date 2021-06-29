use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use arc_swap::ArcSwap;
use futures::Future;
use hyper::StatusCode;
use tokio::io::{AsyncRead, AsyncWrite};
use tower::Service;
use tracing::{debug, warn};

use crate::{config::SharedData, peer_addr::PeerAddr, router::Route, trace::TraceExecutor};

type HyperRequest = hyper::Request<hyper::Body>;
type HyperResponse = hyper::Response<hyper::Body>;
type HttpServer = hyper::server::conn::Http<TraceExecutor>;

#[derive(Clone)]
pub struct HttpService {
    shared_data: Arc<ArcSwap<SharedData>>,
}

impl HttpService {
    pub fn new(shared_data: Arc<ArcSwap<SharedData>>) -> Self {
        HttpService { shared_data }
    }
}

impl Service<HyperRequest> for HttpService {
    type Response = HyperResponse;
    type Error = crate::Error;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: HyperRequest) -> Self::Future {
        let shared = self.shared_data.load();

        Box::pin(async move {
            let resp = match shared.router.recognize(req.uri().path()) {
                Ok(m) => {
                    let route = m.handler();
                    let routes: Vec<&Route> = route
                        .routes
                        .iter()
                        .filter(|r| r.matcher.matchs(&req))
                        .collect();

                    match routes.first() {
                        Some(r) => {
                            let resp = hyper::Response::builder()
                                .body(hyper::Body::from(format!(
                                    "upstream: {:?}",
                                    r.upstream.read().unwrap().name
                                )))
                                .unwrap();
                            resp
                        }
                        None => not_found(),
                    }
                }
                Err(_e) => not_found(),
            };

            Ok(resp)
        })
    }
}

fn not_found() -> HyperResponse {
    hyper::Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(hyper::Body::from("Not Found"))
        .unwrap()
}

#[derive(Clone, Debug)]
pub struct ConnService<S> {
    inner: S,
    server: HttpServer,
    drain: drain::Watch,
}

impl<S> ConnService<S> {
    pub fn new(svc: S, server: HttpServer, drain: drain::Watch) -> Self {
        ConnService {
            inner: svc,
            server,
            drain,
        }
    }
}

impl<I, S> Service<I> for ConnService<S>
where
    I: AsyncRead + AsyncWrite + PeerAddr + Send + Unpin + 'static,
    S: Service<HyperRequest, Response = HyperResponse, Error = crate::Error>
        + Clone
        + Unpin
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = ();
    type Error = crate::Error;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, io: I) -> Self::Future {
        let Self {
            server,
            inner,
            drain,
        } = self.clone();

        Box::pin(async move {
            let mut conn = server.serve_connection(io, inner);
            tokio::select! {
                res = &mut conn => {
                    debug!(?res, "The client is shutting down the connection");
                    res?
                }
                shutdown = drain.signaled() => {
                    debug!("The process is shutting down the connection");
                    Pin::new(&mut conn).graceful_shutdown();
                    shutdown.release_after(conn).await?;
                }
            }
            Ok(())
        })
    }
}
