use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use drain::Watch;
use hyper::http::uri::Scheme;
use hyper::server::conn::Http;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio_rustls::rustls::sign::CertifiedKey;
use tokio_rustls::webpki::DnsName;
use tower::Service;
use tracing::Instrument;

use crate::config::{Config, RegistryConfig};
use crate::error::ConfigError;
use crate::registry::Registry;
use crate::services::ConnService;
use crate::trace::TraceExecutor;

#[derive(Clone)]
pub struct ServerContext {
    pub http_addr: SocketAddr,
    pub https_addr: SocketAddr,
    pub adminapi_addr: Option<SocketAddr>,
    pub certificates: Arc<HashMap<DnsName, CertifiedKey>>,
    pub registry: Registry,

    pub config_notify: Arc<Notify>,
    pub watch: Watch,

    pub config: Arc<Config>,
    pub registry_cfg: Arc<RwLock<RegistryConfig>>,
}

impl ServerContext {
    pub async fn new(cfg: Config, watch: Watch) -> Result<Self, ConfigError> {
        let http_addr = cfg.server.http_addr.parse()?;
        let https_addr = cfg.server.https_addr.parse()?;
        let adminapi_addr = if cfg.admin.enable {
            Some(cfg.admin.adminapi_addr.parse::<SocketAddr>()?)
        } else {
            None
        };

        // load registry
        let registry_config = cfg.registry_provider.load_registry()?;

        let certificates = Arc::new(HashMap::new());
        let registry = Registry::new(&registry_config)?;
        let config = Arc::new(cfg);
        let config_notify = Arc::new(Notify::new());
        let registry_config = Arc::new(RwLock::new(registry_config));

        Ok(ServerContext {
            http_addr,
            https_addr,
            adminapi_addr,
            registry,
            certificates,
            config,
            registry_cfg: registry_config,
            config_notify,
            watch,
        })
    }

    pub fn start_watch_config(&self) {
        let registry_cfg = self.registry_cfg.clone();
        let notify = self.config_notify.clone();
        let shared_data = self.registry.clone();

        tokio::spawn(async move {
            loop {
                notify.notified().await;

                Self::apply_config(registry_cfg.clone(), shared_data.clone());
            }
        });
    }

    pub fn apply_config(registry: Arc<RwLock<RegistryConfig>>, shared_data: Registry) {
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




pub struct Server {
    scheme: Scheme,
    shared_data: Registry,
}

impl Server {
    pub fn new(scheme: Scheme, shared_data: Registry) -> Self {
        Server {
            scheme,
            shared_data,
        }
    }

    pub async fn run(self, addr: SocketAddr, watch: Watch) -> crate::Result<()> {
        let Server {
            scheme,
            shared_data,
        } = self;

        let http = Http::new().with_executor(TraceExecutor::new());

        let listener = TcpListener::bind(addr).await?;

        tracing::info!("server listen on {:?}", addr);

        let conn_svc = ConnService::new(shared_data, scheme, http, watch.clone());

        loop {
            tokio::select! {
                ret = listener.accept() => {
                    tracing::debug!("accepting {:?}", ret);

                    match ret {
                        Ok((stream, remote_addr)) => {
                            let mut conn_svc = conn_svc.clone();
                            let span = tracing::debug_span!("connection", %remote_addr);
                            let _enter = span.enter();
                            let fut = async move {
                                let ret = Service::call(&mut conn_svc, stream).await;
                                tracing::debug!(?ret, "handle connection done");
                            };
                            tokio::spawn(fut.in_current_span());
                        }
                        Err(e) => {
                            tracing::error!("accept failed, {:?}", e);
                        }
                    }
                }
                _shutdown = watch.clone().signaled() => {
                    tracing::info!("stopping accept");
                    break;
                }
            }
        }

        Ok(())
    }
}
