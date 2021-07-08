use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use arc_swap::ArcSwap;
use futures::TryFutureExt;
use hyper::{client::HttpConnector, Client, Method, Request, StatusCode, Uri};
use hyper_rustls::HttpsConnector;
use hyper_timeout::TimeoutConnector;
use serde::{Deserialize, Serialize};

use crate::{
    http::{HyperRequest, HyperResponse},
    upstream::Upstream,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthConfig {
    /// in seconds
    pub slow_threshold: u64,
    /// in seconds
    pub timeout: u64,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Healthiness {
    Healthly,
    Slow(Duration),
    Unresponsive(Option<StatusCode>),
}

pub async fn health_check() {}

pub async fn health_check_one_upstream(upstream: &Upstream) {
    let https = hyper_rustls::HttpsConnector::with_native_roots();
    let mut connector = TimeoutConnector::new(https);
    let timeout = Some(Duration::from_secs(upstream.health_config.timeout));
    connector.set_connect_timeout(timeout);
    connector.set_read_timeout(timeout);
    connector.set_write_timeout(timeout);

    let client: Client<_, hyper::Body> =
        Client::builder().pool_max_idle_per_host(0).build(connector);

    for (endpoint, healthiness) in &upstream.endpoints {
        let uri = Uri::builder()
            .scheme(upstream.scheme.clone())
            .authority(endpoint.addr.as_str())
            .path_and_query(upstream.health_config.path.as_str())
            .build()
            .expect("build upstream uri failed");

        let cfg = upstream.health_config.clone();
        let healthiness = healthiness.clone();
        // tokio::spawn(async move {
        //     detect_endpoint_health(client.clone(), uri, cfg, healthiness).await;
        // });
    }
}

async fn detect_endpoint_health(
    client: Client<TimeoutConnector<HttpsConnector<HttpConnector>>, hyper::Body>,
    uri: Uri,
    cfg: HealthConfig,
    healthiness: ArcSwap<Healthiness>,
) {
    let req = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(hyper::Body::empty())
        .unwrap();

    let begin = Instant::now();

    let health = match client.request(req).await {
        Ok(resp) => {
            let take = begin.elapsed();
            if take > Duration::from_secs(cfg.slow_threshold) {
                Healthiness::Slow(take)
            } else {
                if resp.status().is_success() {
                    Healthiness::Healthly
                } else {
                    Healthiness::Unresponsive(Some(resp.status()))
                }
            }
        }
        Err(err) => Healthiness::Unresponsive(None),
    };

    healthiness.store(Arc::new(health));
}
