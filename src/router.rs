use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::config::RouteConfig;
use crate::error::{upstream_not_found, ConfigError};
use crate::matcher::RouteMatcher;
use crate::plugins::{init_plugin, Plugin};
use crate::upstream::Upstream;

pub type PathRouter = route_recognizer::Router<Vec<Route>>;

#[derive(Clone)]
pub struct Route {
    pub matcher: RouteMatcher,
    pub upstream_id: String,
    pub priority: u32,
    pub plugins: Vec<Arc<Box<dyn Plugin + Send + Sync>>>,
}

impl Route {
    pub fn new(
        cfg: &RouteConfig,
        upstreams: HashMap<String, Arc<RwLock<Upstream>>>,
    ) -> Result<Route, ConfigError> {
        let matcher = RouteMatcher::parse(&cfg.matcher)?;

        if upstreams.get(&cfg.upstream_id).is_none() {
            return Err(upstream_not_found(&cfg.upstream_id));
        }

        let mut plugins = Vec::new();

        // for plugin in &cfg.plugins {
        //     let p = init_plugin(plugin)?;
        //     plugins.push(p);
        // }

        // sort plugin by priority
        // plugins.sort_unstable_by_key(|p| Reverse(p.priority()));

        Ok(Route {
            matcher,
            upstream_id: cfg.upstream_id.to_string(),
            priority: cfg.priority,
            plugins,
        })
    }
}
