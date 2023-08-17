use std::cmp::Reverse;
use std::sync::Arc;

use crate::config::RouteConfig;
use crate::error::ConfigError;
use crate::matcher::RouteMatcher;
use crate::plugins::{init_plugin, Plugin};

pub type PathRouter = pathrouter::Router<Vec<Route>>;

#[derive(Clone)]
pub struct Route {
    pub id: String,
    pub matcher: RouteMatcher,
    pub upstream_id: String,
    pub overwrite_host: bool,
    pub priority: u32,
    pub plugins: Vec<Arc<Box<dyn Plugin + Send + Sync>>>,
}

impl Route {
    pub fn new(cfg: &RouteConfig) -> Result<Route, ConfigError> {
        if cfg.upstream_id.is_empty() {
            return Err(ConfigError::UpstreamNotFound("UpstreamId missing".to_string()));
        }

        let matcher = RouteMatcher::parse(&cfg.matcher)?;

        let mut plugins = Vec::new();

        for (name, config) in &cfg.plugins {
            let p = init_plugin(name, config.config.clone())?;
            plugins.push(p);
        }

        // sort plugin by priority
        plugins.sort_unstable_by_key(|p| Reverse(p.priority()));

        Ok(Route {
            id: cfg.id.clone(),
            matcher,
            overwrite_host: cfg.overwrite_host,
            upstream_id: cfg.upstream_id.to_string(),
            priority: cfg.priority,
            plugins,
        })
    }
}
