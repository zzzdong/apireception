use std::{
    collections::HashMap,
    convert::TryFrom,
    pin::Pin,
    sync::{Arc, RwLock},
    task::{Context, Poll},
};

use futures::Future;
use headers::HeaderValue;
use hyper::{
    header::HOST,
    http::{uri::{Authority, Scheme}, Extensions},
    Uri,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tower::Service;
use tracing::{debug, error};

use crate::{
    config::SharedData,
    http::{bad_gateway},
    peer_addr::PeerAddr,
    router::{PathRouter, Route},
    upstream::Upstream,
};
use crate::{
    context::{GatewayInfo, RequestInfo},
    http::{
        not_found, upstream_unavailable, HttpServer, HyperRequest, HyperResponse,
        ResponseFuture,
    },
};

#[derive(Clone)]
pub struct GatewayService {
    shared_data: SharedData,
}

impl GatewayService {
    pub fn new(shared_data: SharedData) -> Self {
        GatewayService { shared_data }
    }

    pub fn find_route<'a>(router: &'a PathRouter, req: &HyperRequest) -> Option<&'a Route> {
        match router.recognize(req.uri().path()) {
            Ok(m) => {
                let routes = *m.handler();

                let routes: Vec<&Route> = routes.iter().filter(|r| r.matcher.matchs(req)).collect();

                routes.first().cloned()
            }
            Err(_err) => {
                debug!("route not found");
                None
            }
        }
    }

    pub async fn dispatch(
        route: &Route,
        upstreams: &HashMap<String, Arc<RwLock<Upstream>>>,
        mut req: HyperRequest,
    ) -> HyperResponse {
        let request_info = {
            let info = req
                .extensions_mut()
                .get::<RequestInfo>()
                .expect("RemoteInfo must exist");

            info.clone()
        };

        let upstream_id = route.upstream_id.clone();

        let mut ctx = GatewayInfo {
            request_info,
            upstream_id: None,
            extensions: Extensions::new(),
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

        let upstream_id = ctx.upstream_id.clone().unwrap_or(route.upstream_id.clone());

        let (mut client) = match upstreams.get(&upstream_id) {
            Some(upstream) => {
                let upstream = upstream.read().unwrap();
                let healthy_endpoints = upstream.healthy_endpoints();

                (upstream.client.clone())
            }
            None => {
                return upstream_unavailable();
            }
        };

        // do forward
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
        let router = self.shared_data.router.load().clone();
        let upstreams = self.shared_data.upstreams.load().clone();

        Box::pin(async move {
            let found = Self::find_route(&router, &req);
            let resp = match found {
                Some(route) => {
                    let upstreams = &upstreams;
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
    scheme: Scheme,
    server: HttpServer,
    drain: drain::Watch,
}

impl<S> ConnService<S> {
    pub fn new(svc: S, scheme: Scheme, server: HttpServer, drain: drain::Watch) -> Self {
        ConnService {
            inner: svc,
            scheme,
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
            scheme,
            inner,
            drain,
        } = self.clone();

        let remote_addr = io.peer_addr().expect("can not get peer addr");
        let info = RequestInfo::new(scheme, remote_addr);
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

pub trait AppendInfo {
    fn append_info(&mut self, req: HyperRequest);
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
    T: AppendInfo + Clone + Send + Sync + 'static,
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
