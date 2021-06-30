use std::sync::{Arc, RwLock};

use hyper::client::HttpConnector;
use hyper::Body;
use hyper::Client;

use crate::http::not_found;
use crate::http::upstream_all_down;
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
    pub async fn forward_request(
        &self,
        mut req: hyper::Request<hyper::Body>,
    ) -> hyper::Response<Body> {
        let upstream = self.upstream.read().unwrap();

        let working_upstream = upstream.heathy_endpoints();

        if working_upstream.is_empty() {
            return upstream_all_down();
        }

        let ctx = Context {
            upstream_addrs: &working_upstream,
        };

        let uri = upstream.select_upstream(&ctx, &req);

        *req.uri_mut() = uri;

        match self.client.request(req).await {
            Ok(resp) => resp,
            Err(err) => {
                tracing::error!(?err, "forward request failed");
                not_found()
            }
        }
    }
}
