use std::{
    collections::VecDeque,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use hyper::{client::HttpConnector, http::uri::Scheme, Client, Method, Request, Uri};
use hyper_rustls::HttpsConnector;
use hyper_timeout::TimeoutConnector;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{Receiver, Sender};

use crate::{runtime::SharedData, upstream::Upstream};

type HttpClient = Client<TimeoutConnector<HttpsConnector<HttpConnector>>, hyper::Body>;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthConfig {
    /// reqeust timeout in milliseconds
    pub timeout: u64,
    /// request interval in seconds
    pub interval: u64,
    /// request path
    pub path: String,
    /// status code check regex
    pub status_regex: String,
    pub rise: u64,
    pub fall: u64,
    pub default_down: bool,
}

struct HealthChecker {
    shared_data: SharedData,
}

struct UpstreamChecker {
    upstream: Arc<Upstream>,
}

impl UpstreamChecker {
    fn new(upstream: Arc<Upstream>) -> Self {
        UpstreamChecker { upstream }
    }

    async fn start(self) {
        let (tx, rx) = tokio::sync::mpsc::channel::<()>(self.upstream.endpoints.len());
        let client = create_http_client(&self.upstream.health_config);

        for (ep, status_store) in &self.upstream.endpoints {
            let parts = ep.target.clone().into_parts();

            let path = match parts.path_and_query {
                Some(p) => p.to_string() + self.upstream.health_config.path.as_str(),
                None => self.upstream.health_config.path.clone(),
            };

            let uri = Uri::builder()
                .scheme(parts.scheme.unwrap_or(Scheme::HTTP))
                .authority(parts.authority.expect("endpoint authority empty"))
                .path_and_query(self.upstream.health_config.path.as_str())
                .build()
                .expect("build upstream uri failed");
            let health_config = self.upstream.health_config.clone();

            tokio::spawn(Self::check_endpoint(
                health_config,
                status_store.clone(),
                tx.clone(),
                client.clone(),
                uri,
            ));
        }
    }

    async fn check_endpoint(
        cfg: HealthConfig,
        status_store: Arc<RwLock<Healthiness>>,
        statuc_tx: Sender<()>,
        client: HttpClient,
        uri: Uri,
    ) {
        let mut status_ring = StatusRing::new(&cfg);
        // init status
        let status = status_ring.status();
        *status_store.write().unwrap() = status;

        loop {
            // read close signal
            tokio::select! {
                _ = statuc_tx.closed() => {
                    tracing::info!("stop endpoint health check due to channel closed");
                    break;
               }

               else => {
                    // check and set status
                    let status = detect_endpoint_health(client.clone(), uri.clone()).await;
                    let status = status_ring.append(status);

                    let orig_status =  { *status_store.read().unwrap() };
                    if orig_status != status {
                        *status_store.write().unwrap() = status;
                    }
                    // wait for next
                    tokio::time::sleep(Duration::from_millis(cfg.interval)).await;
               }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum Healthiness {
    Up,
    Down,
}

struct StatusRing {
    status: Healthiness,
    raise: usize,
    fall: usize,
    capacity: usize,
    ring: VecDeque<Healthiness>,
}

impl StatusRing {
    pub fn new(cfg: &HealthConfig) -> Self {
        let status = if cfg.default_down {
            Healthiness::Down
        } else {
            Healthiness::Up
        };
        let capacity = (cfg.rise + cfg.fall) as usize;
        StatusRing {
            status,
            capacity,
            raise: cfg.rise as usize,
            fall: cfg.fall as usize,
            ring: VecDeque::with_capacity(capacity),
        }
    }

    pub fn status(&self) -> Healthiness {
        self.status
    }

    pub fn append(&mut self, status: Healthiness) -> Healthiness {
        self.ring.push_back(status);
        if self.ring.len() >= self.capacity {
            self.ring.pop_front();
        }

        match status {
            Healthiness::Down => {
                if self.check_status(status, self.fall) {
                    self.status = status;
                }
            }
            Healthiness::Up => {
                if self.check_status(status, self.raise) {
                    self.status = status;
                }
            }
        }

        self.status
    }

    fn check_status(&self, expect: Healthiness, threshold: usize) -> bool {
        for _ in 0..threshold {
            if Some(&expect) != self.ring.iter().rev().next() {
                return false;
            }
        }
        true
    }
}

fn create_http_client(cfg: &HealthConfig) -> HttpClient {
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .https_or_http()
        .enable_http1()
        .build();
    let mut connector = TimeoutConnector::new(https);
    let timeout = Some(Duration::from_millis(cfg.timeout));
    connector.set_connect_timeout(timeout);
    connector.set_read_timeout(timeout);
    connector.set_write_timeout(timeout);

    let client: Client<_, hyper::Body> =
        Client::builder().pool_max_idle_per_host(0).build(connector);

    client
}

pub async fn health_check() {}

pub async fn health_check_one_upstream(upstream: &Upstream) {
    for (endpoint, healthiness) in &upstream.endpoints {
        let parts = endpoint.target.clone().into_parts();

        let path = match parts.path_and_query {
            Some(p) => p.to_string() + upstream.health_config.path.as_str(),
            None => upstream.health_config.path.clone(),
        };

        let uri = Uri::builder()
            .scheme(parts.scheme.unwrap_or(Scheme::HTTP))
            .authority(parts.authority.expect("endpoint authority error"))
            .path_and_query(upstream.health_config.path.as_str())
            .build()
            .expect("build upstream uri failed");

        let cfg = upstream.health_config.clone();
        // tokio::spawn(async move {
        //     detect_endpoint_health(client.clone(), uri, cfg, healthiness).await;
        // });
    }
}

async fn detect_endpoint_health(client: HttpClient, uri: Uri) -> Healthiness {
    let req = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(hyper::Body::empty())
        .unwrap();

    let begin = Instant::now();

    match client.request(req).await {
        Ok(resp) => {
            if resp.status().is_success() {
                Healthiness::Up
            } else {
                Healthiness::Down
            }
        }
        Err(err) => Healthiness::Down,
    }
}
