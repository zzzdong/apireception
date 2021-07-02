use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use hyper::Uri;
use regex::Regex;
use tower::Service;

use crate::config::PathRewriteConfig;
use crate::config::RouteConfig;
use crate::error::{upstream_not_found, ConfigError};
use crate::http::HyperRequest;
use crate::http::HyperResponse;
use crate::http::RemoteInfo;
use crate::http::{bad_gateway, set_proxy_headers, upstream_unavailable};
use crate::matcher::RouteMatcher;
use crate::upstream::Context;
use crate::upstream::Upstream;

pub type PathRouter = route_recognizer::Router<Vec<Route>>;

#[derive(Clone)]
pub struct Route {
    pub matcher: RouteMatcher,
    pub upstream: Arc<RwLock<Upstream>>,
    pub priority: u32,

    path_rewrite: PathRewrite,
}

impl Route {
    pub fn new(
        cfg: &RouteConfig,
        upstreams: HashMap<String, Arc<RwLock<Upstream>>>,
    ) -> Result<Route, ConfigError> {
        let matcher = RouteMatcher::parse(&cfg.matcher)?;

        let upstream = upstreams
            .get(&cfg.upstream_name)
            .ok_or_else(|| upstream_not_found(&cfg.upstream_name))?
            .clone();

        let path_rewrite = match cfg.path_rewrite {
            PathRewriteConfig::Keep => PathRewrite::Keep,
            PathRewriteConfig::Static(ref s) => PathRewrite::Static(s.to_string()),
            PathRewriteConfig::RegexReplace(ref m, ref r) => {
                let re = Regex::new(m).map_err(|e| ConfigError::Message(e.to_string()))?;
                PathRewrite::RegexReplace(re, r.to_string())
            }
        };

        Ok(Route {
            matcher,
            upstream,
            path_rewrite,
            priority: cfg.priority,
        })
    }

    pub async fn forward_request(&self, mut req: HyperRequest) -> HyperResponse {
        let info = req
            .extensions_mut()
            .remove::<RemoteInfo>()
            .expect("RemoteInfo must exist");

        let remote_addr = info.addr;

        let (scheme, endpoint) = {
            let upstream = self.upstream.read().unwrap();

            let working_upstream = upstream.heathy_endpoints();

            if working_upstream.is_empty() {
                return upstream_unavailable();
            }

            let ctx = Context {
                upstream_addrs: &working_upstream,
                remote_addr: &remote_addr,
            };

            (upstream.scheme.clone(), upstream.select_upstream(&ctx))
        };

        let path = self.path_rewrite(req.uri());

        let uri = Uri::builder()
            .scheme(scheme)
            .authority(endpoint.as_str())
            .path_and_query(path)
            .build()
            .unwrap();

        set_proxy_headers(&mut req, &info);

        let upstream = uri.authority().unwrap();
        *req.uri_mut() = uri.clone();

        let mut client = self.upstream.read().unwrap().client.clone();

        match Service::call(&mut client, req).await {
            Ok(resp) => resp,
            Err(err) => {
                tracing::error!(?err, ?upstream, "forward request failed");
                bad_gateway()
            }
        }
    }

    fn path_rewrite(&self, uri: &Uri) -> String {
        let path_and_query = uri.path_and_query().unwrap();

        let query = path_and_query.query();

        let path = path_and_query.path();

        let path = match self.path_rewrite {
            PathRewrite::Keep => path.to_string(),
            PathRewrite::Static(ref s) => s.to_string(),
            PathRewrite::RegexReplace(ref re, ref rep) => re.replace(path, rep).to_string(),
        };

        match query {
            Some(q) => path + "?" + q,
            None => path,
        }
    }
}

#[derive(Debug, Clone)]
enum PathRewrite {
    Keep,
    Static(String),
    RegexReplace(regex::Regex, String),
}
