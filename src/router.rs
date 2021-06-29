use std::sync::{Arc, RwLock};
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::{Path, PathBuf},
};


use crate::matcher::RouteMatcher;
use crate::upstream::Upstream;


pub type PathRouter = route_recognizer::Router<PathRoute>;

pub struct PathRoute {
    pub routes: Vec<Route>,
}


pub struct Route {
    pub matcher: RouteMatcher,
    pub upstream: Arc<RwLock<Upstream>>,
}
