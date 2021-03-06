use std::cmp::Reverse;
use std::sync::Arc;

use crate::config::RouteConfig;
use crate::error::ConfigError;
use crate::matcher::RouteMatcher;
use crate::plugins::{init_plugin, Plugin};

pub type PathRouter = route_recognizer::Router<Vec<Route>>;

#[derive(Clone)]
pub struct Route {
    pub matcher: RouteMatcher,
    pub upstream_id: String,
    pub priority: u32,
    pub plugins: Vec<Arc<Box<dyn Plugin + Send + Sync>>>,
}

impl Route {
    pub fn new(cfg: &RouteConfig) -> Result<Route, ConfigError> {
        let matcher = RouteMatcher::parse(&cfg.matcher)?;

        let mut plugins = Vec::new();

        for (name, config) in &cfg.plugins {
            let p = init_plugin(name, config.config.clone())?;
            plugins.push(p);
        }

        // sort plugin by priority
        plugins.sort_unstable_by_key(|p| Reverse(p.priority()));

        Ok(Route {
            matcher,
            upstream_id: cfg.upstream_id.to_string(),
            priority: cfg.priority,
            plugins,
        })
    }
}
