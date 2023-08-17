use std::collections::HashMap;
use std::convert::TryInto;
use std::sync::{Arc, RwLock};

use hyper::Uri;

use crate::config::UpstreamConfig;

use crate::error::ConfigError;
use crate::forwarder::HttpClient;
use crate::health::{HealthConfig, Healthiness};
use crate::load_balance::*;
use crate::registry::Endpoint;

pub type UpstreamMap = HashMap<String, Arc<RwLock<Upstream>>>;

pub struct Upstream {
    pub id: String,
    pub name: String,
    pub client: HttpClient,
    pub strategy: Arc<Box<dyn LoadBalanceStrategy>>,
    pub endpoints: Vec<(Endpoint, Arc<RwLock<Healthiness>>)>,
    pub health_config: HealthConfig,
}

impl Upstream {
    pub fn new(cfg: &UpstreamConfig) -> Result<Self, ConfigError> {
        let mut endpoints = Vec::new();
        for ep in &cfg.endpoints {
            let uri = ep.addr.parse::<Uri>()?;
            endpoints.push((
                Endpoint::new(uri, ep.weight.try_into().unwrap()),
                Arc::new(RwLock::new(Healthiness::Up)),
            ));
        }

        let strategy: Arc<Box<dyn LoadBalanceStrategy>> = match cfg.strategy.as_str() {
            "random" => Arc::new(Box::new(Random::new())),
            "weighted" => Arc::new(Box::new(WeightedRandom::new())),
            "least_request" => Arc::new(Box::new(LeastRequest::new())),
            s => {
                return Err(ConfigError::UnknownLBStrategy(s.to_string()));
            }
        };

        let client = HttpClient::new();

        Ok(Upstream {
            id: cfg.id.clone(),
            name: cfg.name.clone(),
            endpoints,
            client,
            strategy,
            health_config: cfg.health_check.clone(),
        })
    }

    pub fn healthy_endpoints(&self) -> Vec<&Endpoint> {
        self.endpoints
            .iter()
            .filter(|(endpoint, healthiness)| {
                (endpoint.weight != 0) && (*healthiness.read().unwrap() == Healthiness::Up)
            })
            .map(|(endpoint, _)| endpoint)
            .collect::<Vec<_>>()
    }

    pub fn all_endpoints(&self) -> Vec<&Endpoint> {
        self.endpoints
            .iter()
            .filter(|(endpoint, _healthiness)| endpoint.weight != 0)
            .map(|(endpoint, _)| endpoint)
            .collect::<Vec<_>>()
    }

    // pub fn select_endpoint(&self, ctx: &GatewayContext, req: &HyperRequest) -> Option<String> {
    //     let mut available_endpoints = self.healthy_endpoints();
    //     if available_endpoints.is_empty() {
    //         available_endpoints = self.all_endpoints();
    //     }

    //     ctx.available_endpoints = available_endpoints.into_iter().map(|item|item.clone()).collect();

    //     let endpoint = self.strategy.select_endpoint(ctx, req).to_string();

    //     Some(endpoint)
    // }
}
