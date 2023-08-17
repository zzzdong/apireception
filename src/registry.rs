use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    iter::FromIterator,
    path::Path,
    sync::{Arc, RwLock},
    time::SystemTime,
};

use hyper::Uri;
use left_right::{Absorb, ReadHandle, WriteHandle, ReadGuard};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::{
    config::{RegistryProvider, RouteConfig, UpstreamConfig},
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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RegistryConfig {
    #[serde(default)]
    pub routes: Vec<RouteConfig>,
    #[serde(default)]
    pub upstreams: Vec<UpstreamConfig>,
}

impl RegistryConfig {
    pub fn load(provider: &RegistryProvider) -> Result<Self, ConfigError> {
        match provider {
            RegistryProvider::Etcd(cfg) => {
                unimplemented!()
            }
            RegistryProvider::File(cfg) => RegistryConfig::load_file(&cfg.path),
        }
    }

    // pub async fn load_db(&mut self, db: Database) -> Result<(), ConfigError> {
    //     // load routes
    //     let routes_col = db.collection::<RouteConfig>(COL_ROUTES);

    //     let cursor = routes_col.find(None, None).await?;

    //     let routes: Vec<RouteConfig> = cursor.try_collect().await?;

    //     self.routes = routes;

    //     // load upstreams
    //     let upstreams_col = db.collection::<UpstreamConfig>(COL_UPSTREAMS);

    //     let cursor = upstreams_col.find(None, None).await?;

    //     let upstreams: Vec<UpstreamConfig> = cursor.try_collect().await?;

    //     self.upstreams = upstreams;

    //     Ok(())
    // }

    // pub async fn dump_db(&mut self, db: Database) -> Result<(), ConfigError> {
    //     // insert routes
    //     let routes_col = db.collection::<RouteConfig>(COL_ROUTES);

    //     let _ret = routes_col.insert_many(self.routes.clone(), None).await?;

    //     // insert upstreams
    //     let upstreams_col = db.collection::<UpstreamConfig>(COL_UPSTREAMS);

    //     let _ret = upstreams_col
    //         .insert_many(self.upstreams.clone(), None)
    //         .await?;

    //     Ok(())
    // }

    pub fn load_file(path: impl AsRef<Path>) -> Result<RegistryConfig, ConfigError> {
        crate::config::load_file(path)
    }

    pub fn dump_file(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
        crate::config::dump_file(self, path)
    }
}

#[derive(Clone, Default)]
pub struct Registry {
    pub config: RegistryConfig,
    pub router: PathRouter,
    pub upstreams: UpstreamMap,
}

impl Registry {
    pub fn new(provider: &RegistryProvider) -> Result<Self, ConfigError> {
        let config = RegistryConfig::load(provider)?;

        let router = Self::build_router(&config)?;
        let upstreams = Self::build_upstream_map(&config)?;

        Ok(Registry {
            config,
            router,
            upstreams,
        })
    }

    pub(crate) fn new_reader_writer() -> (RegistryReader, RegistryWriter) {
        let (write, read) = left_right::new::<Registry, RegistryOp>();

        (RegistryReader(read), RegistryWriter(write))
    }

    pub fn reload(&mut self, cfg: RegistryConfig) -> Result<(), ConfigError> {
        let router = Self::build_router(&cfg)?;
        let upstreams = Self::build_upstream_map(&cfg)?;

        self.config = cfg;
        self.router = router;
        self.upstreams = upstreams;

        Ok(())
    }

    pub fn add_route(&mut self, cfg: &RouteConfig) -> Result<(), ConfigError> {
        let route = Route::new(cfg)?;

        // check upstream
        self.upstreams
            .values()
            .find(|item| item.read().unwrap().id == route.upstream_id)
            .ok_or(ConfigError::UpstreamNotFound(route.upstream_id.clone()))?;

        for uri in &cfg.uris {
            let endpoint = self.router.at_or_default(uri);
            endpoint.push(route.clone());
            endpoint.sort_unstable_by_key(|r| Reverse(r.priority))
        }

        Ok(())
    }

    pub fn delete_route(&mut self, cfg: &RouteConfig) -> Result<(), ConfigError> {
        let route = Route::new(cfg)?;

        for uri in &cfg.uris {
            let endpoint = self.router.at_or_default(uri);

            endpoint.retain(|item| item.id != route.id);
            endpoint.sort_unstable_by_key(|r| Reverse(r.priority))
        }

        Ok(())
    }

    pub fn add_upstream(&mut self, cfg: &UpstreamConfig) -> Result<(), ConfigError> {
        let upstream = Upstream::new(cfg)?;

        self.upstreams
            .insert(upstream.id.clone(), Arc::new(RwLock::new(upstream)));
        Ok(())
    }

    pub fn delete_upstream(&mut self, upstream: &UpstreamConfig) -> Result<(), ConfigError> {
        self.upstreams.remove(&upstream.id);
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

    fn build_upstream_map(cfg: &RegistryConfig) -> Result<UpstreamMap, ConfigError> {
        let mut upstreams: UpstreamMap = HashMap::new();

        for u in &cfg.upstreams {
            let upstream = Upstream::new(u)?;
            upstreams.insert(u.name.clone(), Arc::new(RwLock::new(upstream)));
        }

        Ok(upstreams)
    }



    // pub fn start_watch_notify(&self, notify: Arc<Notify>) {
    //     let config = self.config.clone();
    //     let registry = self.clone();

    //     tokio::spawn(async move {
    //         loop {
    //             notify.notified().await;

    //             Self::apply_config(config.clone(), registry.clone());
    //         }
    //     });
    // }

    fn apply_config(cfg: Arc<RwLock<RegistryConfig>>, mut registry: Registry) {
        let cfg = cfg.read().unwrap();
        match registry.reload(cfg.clone()) {
            Ok(_) => {
                let mut path = std::env::temp_dir();
                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap();
                let filename = format!("apireception-config-{:?}.yaml", now.as_secs_f32());

                path.push("apirecption");

                path.push(filename);

                cfg.dump_file(path).unwrap();
            }
            Err(err) => {
                tracing::error!(%err, "apply config failed")
            }
        }
    }
}

#[derive(Debug)]
pub enum RegistryOp {
    Reload(RegistryConfig),
    AddRoute(RouteConfig),
    DeleteRoute(RouteConfig),
    AddUpstream(UpstreamConfig),
    DeleteUpstream(UpstreamConfig),
}

impl Absorb<RegistryOp> for Registry {
    fn absorb_first(&mut self, operation: &mut RegistryOp, other: &Self) {
        match operation {
            RegistryOp::Reload(cfg) => {
                self.reload(cfg.clone());
            }
            RegistryOp::AddRoute(cfg) => {
                self.add_route(cfg);
            }
            RegistryOp::DeleteRoute(cfg) => {
                self.delete_route(cfg);
            }
            RegistryOp::AddUpstream(cfg) => {
                self.add_upstream(cfg);
            }
            RegistryOp::DeleteUpstream(cfg) => {
                self.delete_upstream(cfg);
            }
        }
    }

    fn sync_with(&mut self, first: &Self) {
        *self = first.clone();
    }
}


pub struct RegistryWriter(WriteHandle<Registry, RegistryOp>);

impl RegistryWriter {
    pub fn load_config(&mut self, conf: RegistryConfig) {
        self.0.append(RegistryOp::Reload(conf));
    }


    pub fn publish(&mut self) {
        self.0.publish();
    }
}

#[derive(Clone)]
pub struct RegistryReader(ReadHandle<Registry>);

impl RegistryReader {
    pub fn get(&self) -> ReadGuard<Registry> {
        self.0.enter().expect("get failed")
    }

    // pub fn get_config(&self) -> &RegistryConfig {
    //     self.0.enter().map(|guard| &guard.config).expect("get failed")
    // }
}


