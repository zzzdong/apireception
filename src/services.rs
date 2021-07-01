use std::{
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use arc_swap::ArcSwap;
use futures::Future;

use tokio::io::{AsyncRead, AsyncWrite};
use tower::Service;
use tracing::{debug, warn};

use crate::http::{not_found, HttpServer, HyperRequest, HyperResponse, ResponseFuture};
use crate::{config::SharedData, peer_addr::PeerAddr, router::Route};

#[derive(Clone)]
pub struct GatewayService {
    shared_data: Arc<ArcSwap<SharedData>>,
}

impl GatewayService {
    pub fn new(shared_data: Arc<ArcSwap<SharedData>>) -> Self {
        GatewayService { shared_data }
    }
}

impl Service<HyperRequest> for GatewayService {
    type Response = HyperResponse;
    type Error = crate::Error;
    type Future = ResponseFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: HyperRequest) -> Self::Future {
        let shared = self.shared_data.load();

        Box::pin(async move {
            let resp = match shared.router.recognize(req.uri().path()) {
                Ok(m) => {
                    let routes = *m.handler();

                    let routes: Vec<&Route> =
                        routes.iter().filter(|r| r.matcher.matchs(&req)).collect();

                    match routes.first() {
                        Some(route) => {
                            let resp = route.forward_request(req).await;
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
    type Future = Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, io: I) -> Self::Future {
        let Self {
            server,
            inner,
            drain,
        } = self.clone();

        let addr = io.peer_addr().expect("can not get peer addr");
        let info = RemoteInfo::new(addr);
        let svc = RemoteInfoService::new(inner, info);

        Box::pin(async move {
            let mut conn = server.serve_connection(io, svc);
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

#[derive(Debug, Clone)]
pub(crate) struct RemoteInfo {
    pub addr: SocketAddr,
}

impl RemoteInfo {
    pub fn new(addr: SocketAddr) -> Self {
        RemoteInfo { addr }
    }
}

#[derive(Clone, Debug)]
struct RemoteInfoService<S> {
    inner: S,
    info: RemoteInfo,
}

impl<S> RemoteInfoService<S> {
    pub fn new(inner: S, info: RemoteInfo) -> Self {
        RemoteInfoService { inner, info }
    }
}

impl<S> Service<HyperRequest> for RemoteInfoService<S>
where
    S: Service<HyperRequest, Response = HyperResponse, Error = crate::Error>
        + Clone
        + Unpin
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = HyperResponse;
    type Error = crate::Error;
    type Future = ResponseFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: HyperRequest) -> Self::Future {
        let RemoteInfoService { mut inner, info } = self.clone();

        req.extensions_mut().insert(info);

        Box::pin(Service::call(&mut inner, req))
    }
}
