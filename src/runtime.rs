use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, RwLock},
    time::SystemTime,
};

use arc_swap::ArcSwap;
use drain::Watch;
use hyper::Uri;
use tokio::sync::Notify;
use tokio_rustls::{rustls::sign::CertifiedKey, webpki::DnsName};

use crate::{
    config::{Config, Registry},
    error::ConfigError,
    router::PathRouter,
    upstream::UpstreamMap,
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
pub struct RuntimeConfig {
    pub http_addr: SocketAddr,
    pub https_addr: SocketAddr,
    pub adminapi_addr: Option<SocketAddr>,
    pub certificates: Arc<HashMap<DnsName, CertifiedKey>>,
    pub shared_data: SharedData,

    pub config_notify: Arc<Notify>,
    pub watch: Watch,

    pub config: Arc<Config>,
    pub registry: Arc<RwLock<Registry>>,
}

impl RuntimeConfig {
    pub async fn new(cfg: Config, watch: Watch) -> Result<Self, ConfigError> {
        let http_addr = cfg.server.http_addr.parse()?;
        let https_addr = cfg.server.https_addr.parse()?;
        let adminapi_addr = if cfg.admin.enable {
            Some(cfg.admin.adminapi_addr.parse::<SocketAddr>()?)
        } else {
            None
        };

        // load registry
        let registry = cfg.registry_provider.load_registry()?;

        let certificates = Arc::new(HashMap::new());
        let shared_data = SharedData::new(&registry)?;
        let config = Arc::new(cfg);
        let config_notify = Arc::new(Notify::new());
        let registry = Arc::new(RwLock::new(registry));

        Ok(RuntimeConfig {
            http_addr,
            https_addr,
            adminapi_addr,
            shared_data,
            certificates,
            config,
            registry,
            config_notify,
            watch,
        })
    }

    pub fn start_watch_config(&self) {
        let registry = self.registry.clone();
        let notify = self.config_notify.clone();
        let shared_data = self.shared_data.clone();

        tokio::spawn(async move {
            loop {
                notify.notified().await;

                Self::apply_config(registry.clone(), shared_data.clone());
            }
        });
    }

    pub fn apply_config(registry: Arc<RwLock<Registry>>, shared_data: SharedData) {
        let registry = registry.read().unwrap();
        match shared_data.reload(&registry) {
            Ok(_) => {
                let mut path = std::env::temp_dir();
                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap();
                let filename = format!("apireception-config-{:?}.yaml", now.as_secs_f32());

                path.push(filename);

                registry.dump_file(path).unwrap();
            }
            Err(err) => {
                tracing::error!(%err, "apply config failed")
            }
        }
    }
}

#[derive(Clone)]
pub struct SharedData {
    pub router: Arc<ArcSwap<PathRouter>>,
    pub upstreams: Arc<ArcSwap<UpstreamMap>>,
}

impl SharedData {
    pub fn new(registry: &Registry) -> Result<Self, ConfigError> {
        let router = registry.build_router()?;
        let upstreams = registry.build_upstream_map()?;

        Ok(SharedData {
            router: Arc::new(ArcSwap::new(Arc::new(router))),
            upstreams: Arc::new(ArcSwap::new(Arc::new(upstreams))),
        })
    }

    pub fn reload(&self, registry: &Registry) -> Result<(), ConfigError> {
        let router = registry.build_router()?;
        let upstreams = registry.build_upstream_map()?;

        self.router.store(Arc::new(router));
        self.upstreams.store(Arc::new(upstreams));

        Ok(())
    }
}
