use std::{
    collections::HashMap,
    convert::TryFrom,
    pin::Pin,
    sync::{Arc, RwLock},
    task::{Context, Poll},
};

use arc_swap::{ArcSwap, Cache};
use futures::Future;
use hyper::{http::uri::Authority, Uri};
use tokio::io::{AsyncRead, AsyncWrite};
use tower::Service;
use tracing::{debug, error};

use crate::{
    config::SharedData,
    http::{append_proxy_headers, bad_gateway},
    peer_addr::PeerAddr,
    router::{PathRouter, Route},
    upstream::Upstream,
};
use crate::{
    context::GatewayContext,
    http::{
        not_found, upstream_unavailable, HttpServer, HyperRequest, HyperResponse, RemoteInfo,
        ResponseFuture,
    },
};

#[derive(Clone)]
pub struct GatewayService {
    shared_data: Arc<ArcSwap<SharedData>>,
}

impl GatewayService {
    pub fn new(shared_data: Arc<ArcSwap<SharedData>>) -> Self {
        GatewayService { shared_data }
    }

    pub fn find_route<'a>(router: &'a PathRouter, req: &HyperRequest) -> Option<&'a Route> {
        match router.recognize(req.uri().path()) {
            Ok(m) => {
                let routes = *m.handler();

                let routes: Vec<&Route> =
                    routes.iter().filter(|r| r.matcher.matchs(&req)).collect();

                routes.first().cloned()
            }
            Err(err) => {
                error!(%err, "find route failed");
                None
            }
        }
    }

    pub async fn dispatch(
        route: &Route,
        upstreams: &HashMap<String, Arc<RwLock<Upstream>>>,
        mut req: HyperRequest,
    ) -> HyperResponse {
        let info = req
            .extensions_mut()
            .remove::<RemoteInfo>()
            .expect("RemoteInfo must exist");

        let remote_addr = info.addr;

        let mut ctx = GatewayContext {
            remote_addr,
            upstream_id: route.upstream_id.clone(),
        };

        for plugin in &route.plugins {
            match plugin.on_access(&mut ctx, req) {
                Ok(r) => {
                    req = r;
                }
                Err(resp) => {
                    return resp;
                }
            }
        }

        let mut client = match upstreams.get(&ctx.upstream_id) {
            Some(upstream) => {
                let mut parts = req.uri().clone().into_parts();

                let upstream = upstream.read().unwrap();
                parts.scheme = Some(upstream.scheme.clone());

                let authority = upstream.select_upstream(&ctx);
                let authority = Authority::try_from(authority.as_str()).ok();
                parts.authority = authority;

                *req.uri_mut() = Uri::from_parts(parts).expect("build uri failed");

                upstream.client.clone()
            }
            None => {
                return upstream_unavailable();
            }
        };

        append_proxy_headers(&mut req, &info);

        let mut resp = match Service::call(&mut client, req).await {
            Ok(resp) => resp,
            Err(err) => {
                error!(?err, "forward request failed");
                bad_gateway()
            }
        };

        for plugin in &route.plugins {
            resp = plugin.after_forward(&mut ctx, resp);
        }

        resp
    }
}

impl Service<HyperRequest> for GatewayService {
    type Response = HyperResponse;
    type Error = crate::Error;
    type Future = ResponseFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: HyperRequest) -> Self::Future {
        let mut shared = Cache::new(self.shared_data.clone());

        Box::pin(async move {
            let shared = shared.load();
            let found = Self::find_route(&shared.router, &req);
            let resp = match found {
                Some(route) => {
                    let upstreams = &shared.upstreams;
                    let resp = Self::dispatch(route, upstreams, req).await;
                    resp
                }
                None => not_found(),
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
        let svc = AppendInfoService::new(inner, info);

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

#[derive(Clone, Debug)]
struct AppendInfoService<S, T> {
    inner: S,
    info: T,
}

impl<S, T> AppendInfoService<S, T> {
    pub fn new(inner: S, info: T) -> Self {
        AppendInfoService { inner, info }
    }
}

impl<S, T> Service<HyperRequest> for AppendInfoService<S, T>
where
    S: Service<HyperRequest, Response = HyperResponse, Error = crate::Error>
        + Clone
        + Unpin
        + Send
        + 'static,
    S::Future: Send + 'static,
    T: Clone + Send + Sync + 'static,
{
    type Response = HyperResponse;
    type Error = crate::Error;
    type Future = ResponseFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: HyperRequest) -> Self::Future {
        let AppendInfoService { mut inner, info } = self.clone();

        req.extensions_mut().insert(info);

        Box::pin(Service::call(&mut inner, req))
    }
}
