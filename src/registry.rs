use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    iter::FromIterator,
    sync::{Arc, RwLock},
};

use arc_swap::ArcSwap;

use hyper::Uri;

use crate::{
    config::RegistryConfig,
    error::{upstream_not_found, ConfigError},
    router::{PathRouter, Route},
    upstream::{Upstream, UpstreamMap},
};

#[derive(Debug, Clone)]
pub struct Endpoint {
    pub target: Uri,
    pub weight: usize,
}

impl Endpoint {
    pub fn new(target: Uri, weight: usize) -> Self {
        Endpoint { target, weight }
    }
}

#[derive(Clone)]
pub struct Registry {
    pub router: Arc<ArcSwap<PathRouter>>,
    pub upstreams: Arc<ArcSwap<UpstreamMap>>,
}

impl Registry {
    pub fn new(cfg: &RegistryConfig) -> Result<Self, ConfigError> {
        let router = Self::build_router(cfg)?;
        let upstreams = Self::build_upstream_map(cfg)?;

        Ok(Registry {
            router: Arc::new(ArcSwap::new(Arc::new(router))),
            upstreams: Arc::new(ArcSwap::new(Arc::new(upstreams))),
        })
    }

    pub fn reload(&self, cfg: &RegistryConfig) -> Result<(), ConfigError> {
        let router = Self::build_router(cfg)?;
        let upstreams = Self::build_upstream_map(cfg)?;

        self.router.store(Arc::new(router));
        self.upstreams.store(Arc::new(upstreams));

        Ok(())
    }

    fn build_router(cfg: &RegistryConfig) -> Result<PathRouter, ConfigError> {
        let mut router = PathRouter::new();

        let upstream_set: HashSet<&str> =
            HashSet::from_iter(cfg.upstreams.iter().map(|up| up.id.as_str()));

        for r in &cfg.routes {
            upstream_set
                .get(r.upstream_id.as_str())
                .ok_or_else(|| upstream_not_found(&r.upstream_id))?;

            let route = Route::new(r)?;

            for uri in &r.uris {
                let endpoint = router.at_or_default(uri);
                endpoint.push(route.clone());
                endpoint.sort_unstable_by_key(|r| Reverse(r.priority))
            }
        }

        Ok(router)
    }

    pub fn build_upstream_map(cfg: &RegistryConfig) -> Result<UpstreamMap, ConfigError> {
        let mut upstreams: UpstreamMap = HashMap::new();

        for u in &cfg.upstreams {
            let upstream = Upstream::new(u)?;
            upstreams.insert(u.name.clone(), Arc::new(RwLock::new(upstream)));
        }

        Ok(upstreams)
    }
}
