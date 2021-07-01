use std::sync::{Arc, RwLock};

use hyper::client::HttpConnector;
use hyper::Body;
use hyper::Client;

use crate::http::HyperRequest;
use crate::http::HyperResponse;
use crate::http::RemoteInfo;
use crate::http::{set_proxy_headers, bad_gateway, upstream_unavailable};
use crate::matcher::RouteMatcher;
use crate::upstream::Context;
use crate::upstream::Upstream;

pub type PathRouter = route_recognizer::Router<Vec<Route>>;

#[derive(Clone)]
pub struct Route {
    pub matcher: RouteMatcher,
    pub upstream: Arc<RwLock<Upstream>>,
    pub priority: u32,
    pub client: Client<HttpConnector, Body>,
}

impl Route {
    pub async fn forward_request(&self, mut req: HyperRequest) -> HyperResponse {
        let info = req
            .extensions_mut()
            .remove::<RemoteInfo>()
            .expect("RemoteInfo not found");

        let remote_addr = info.addr;

        let uri = {
            let upstream = self.upstream.read().unwrap();

            let working_upstream = upstream.heathy_endpoints();

            if working_upstream.is_empty() {
                return upstream_unavailable();
            }

            let ctx = Context {
                upstream_addrs: &working_upstream,
                remote_addr: &remote_addr,
            };

            upstream.select_upstream(&ctx, &req)
        };

        set_proxy_headers(&mut req, &info);

        let upstream = uri.authority().unwrap();
        *req.uri_mut() = uri.clone();

        match self.client.request(req).await {
            Ok(resp) => resp,
            Err(err) => {
                tracing::error!(?err, ?upstream, "forward request failed");
                bad_gateway()
            }
        }
    }
}
