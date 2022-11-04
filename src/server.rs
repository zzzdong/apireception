use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use drain::Watch;
use hyper::http::uri::Scheme;
use hyper::server::conn::Http;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio_rustls::rustls::sign::CertifiedKey;
use tokio_rustls::webpki::DnsName;
use tower::Service;
use tracing::Instrument;

use crate::config::Config;
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

    pub registry_notify: Arc<Notify>,
    pub watch: Watch,

    pub config: Arc<Config>,
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
        let registry = Registry::new(&cfg.registry_provider)?;

        let certificates = Arc::new(HashMap::new());
        let registry_notify = Arc::new(Notify::new());
        let config = Arc::new(cfg);

        Ok(ServerContext {
            http_addr,
            https_addr,
            adminapi_addr,
            registry,
            certificates,
            config,
            registry_notify,
            watch,
        })
    }

    pub fn start_watch_registry(&self) {
        self.registry
            .start_watch_notify(self.registry_notify.clone());
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
