use std::{
    collections::HashMap,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, RwLock},
    task::{Context, Poll},
};

use futures::Future;
use hyper::http::uri::Scheme;
use tokio::io::{AsyncRead, AsyncWrite};
use tower::Service;
use tracing::{debug, error};

use crate::{
    context::GatewayContext,
    http::{
        not_found, upstream_unavailable, HttpServer, HyperRequest, HyperResponse, ResponseFuture,
    },
    registry::{Endpoint, RegistryReader},
};
use crate::{
    forwarder::Fowarder,
    http::bad_gateway,
    peer_addr::PeerAddr,
    router::{PathRouter, Route},
    upstream::Upstream,
};

#[derive(Clone)]
pub struct GatewayService {
    registry_reader: RegistryReader,
    remote_addr: Option<SocketAddr>,
    scheme: Scheme,
}

impl GatewayService {
    pub fn new(
        registry_reader: RegistryReader,
        remote_addr: Option<SocketAddr>,
        scheme: Scheme,
    ) -> Self {
        GatewayService {
            registry_reader,
            remote_addr,
            scheme,
        }
    }

    pub fn find_route<'a>(router: &'a PathRouter, req: &HyperRequest) -> Option<&'a Route> {
        match router.route(req.uri().path()) {
            Some((endpoint, _params)) => {
                let routes: Vec<&Route> =
                    endpoint.iter().filter(|r| r.matcher.matchs(req)).collect();

                routes.first().cloned()
            }
            None => {
                debug!("route not found");
                None
            }
        }
    }

    pub async fn dispatch(
        mut ctx: GatewayContext,
        route: &Route,
        upstreams: &HashMap<String, Arc<RwLock<Upstream>>>,
        mut req: HyperRequest,
    ) -> HyperResponse {
        ctx.overwrite_host = route.overwrite_host;
        ctx.route_id = Some(route.id.clone());
        ctx.upstream_id = Some(route.upstream_id.clone());

        // before forward
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

        // fallback to route.upstream_id
        let upstream_id = ctx.upstream_id.clone().unwrap_or(route.upstream_id.clone());
        ctx.upstream_id = Some(upstream_id.clone());

        let mut forwarder = match upstreams.get(&upstream_id) {
            Some(upstream) => {
                let upstream = upstream.read().unwrap();
                let healthy_endpoints = upstream.healthy_endpoints();
                let available_endpoints = if healthy_endpoints.is_empty() {
                    upstream.all_endpoints()
                } else {
                    healthy_endpoints
                };

                let available_endpoints = available_endpoints
                    .into_iter()
                    .cloned()
                    .collect::<Vec<Endpoint>>();

                ctx.available_endpoints = available_endpoints;

                Fowarder::new(upstream.client.clone(), upstream.strategy.clone())
            }
            None => {
                return upstream_unavailable();
            }
        };

        // do forward
        let mut resp = match forwarder.forward(&mut ctx, req).await {
            Ok(resp) => resp,
            Err(err) => {
                error!(?err, "forward request failed");
                bad_gateway()
            }
        };

        // after forward
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
        debug!("incoming request:{:?} from {:?}", &req, &self.remote_addr);

        let ctx = GatewayContext::new(self.remote_addr, self.scheme.clone(), &req);

        let router = self.registry_reader.get().router.clone();
        let upstreams = self.registry_reader.get().upstreams.clone();

        Box::pin(async move {
            let found = Self::find_route(&router, &req);
            let resp = match found {
                Some(route) => Self::dispatch(ctx, route, &upstreams, req).await,
                None => not_found(),
            };

            Ok(resp)
        })
    }
}

#[derive(Clone)]
pub struct ConnService {
    scheme: Scheme,
    server: HttpServer,
    drain: drain::Watch,
    registry_reader: RegistryReader,
}

impl ConnService {
    pub fn new(
        registry_reader: RegistryReader,
        scheme: Scheme,
        server: HttpServer,
        drain: drain::Watch,
    ) -> Self {
        ConnService {
            scheme,
            server,
            drain,
            registry_reader,
        }
    }
}

impl<I> Service<I> for ConnService
where
    I: AsyncRead + AsyncWrite + PeerAddr + Send + Unpin + 'static,
{
    type Response = ();
    type Error = crate::Error;
    type Future = Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, io: I) -> Self::Future {
        let Self {
            registry_reader,
            server,
            scheme,
            drain,
        } = self.clone();

        let remote_addr = io.peer_addr().ok();

        let svc = GatewayService::new(registry_reader, remote_addr, scheme);

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
