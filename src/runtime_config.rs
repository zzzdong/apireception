use std::sync::Arc;
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use arc_swap::ArcSwap;
use hyper::{Body, Client};
use hyper::client::HttpConnector;
use serde::{Deserialize, Serialize};
use tokio_rustls::{rustls::sign::CertifiedKey, webpki::DNSName};
use route_recognizer::Router as PathRouter;

use crate::config::Upstream;
use crate::matcher::RouteMatcher;

pub struct RuntimeConfig {
    pub http_addrs: Vec<SocketAddr>,
    pub https_addrs: Vec<SocketAddr>,
    pub shared_data: SharedData,
    pub certificates: HashMap<DNSName, CertifiedKey>,
}

pub struct SharedData {
    router: Router,
    upstreams: Vec<Arc<ArcSwap<Upstream>>>,
}

pub struct Router {
    router: PathRouter<PathRoute>,
}


struct PathRoute {
    routes: Vec<Route>,
}

struct Route {
    matcher: RouteMatcher,
}
