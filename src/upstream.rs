use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use hyper::http::uri::{Scheme, Uri};
use rand::{thread_rng, Rng};


use crate::config::{Endpoint, UpstreamConfig};
use crate::context::GatewayInfo;
use crate::error::ConfigError;
use crate::health::{HealthConfig, Healthiness};
use crate::gateway_client::GatewayClient;
use crate::load_balance::{LoadBalanceStrategy, Random, WeightedRandom, LeastRequest, Context};

pub type UpstreamMap = HashMap<String, Arc<RwLock<Upstream>>>;

pub struct Upstream {
    pub name: String,
    pub scheme: Scheme,
    pub client: GatewayClient,
    pub endpoints: Vec<(Endpoint, Arc<RwLock<Healthiness>>)>,
    pub health_config: HealthConfig,
}

impl Upstream {
    pub fn new(cfg: &UpstreamConfig) -> Result<Self, ConfigError> {
        let endpoints = cfg
            .endpoints
            .iter()
            .map(|ep| (ep.clone(), Arc::new(RwLock::new(Healthiness::Up))))
            .collect();

        let scheme = if cfg.is_https {
            Scheme::HTTPS
        } else {
            Scheme::HTTP
        };

        let strategy: Arc<Box<dyn LoadBalanceStrategy>> = match cfg.strategy.as_str() {
            "random" => Arc::new(Box::new(Random::new())),
            "weighted" => Arc::new(Box::new(WeightedRandom::new())),
            "least_request" => Arc::new(Box::new(LeastRequest::new())),
            s => {
                return Err(ConfigError::UnknownLBStrategy(s.to_string()));
            }
        };

        let client = GatewayClient::new(strategy);

        Ok(Upstream {
            name: cfg.name.clone(),
            endpoints,
            scheme,
            client,
            health_config: cfg.health_check.clone(),
        })
    }

    pub fn healthy_endpoints(&self) -> Vec<Endpoint> {
        self.endpoints
            .iter()
            .filter(|(endpoint, healthiness)| {
                (endpoint.weight != 0) && (*healthiness.read().unwrap() == Healthiness::Up)
            })
            .map(|(endpoint, _)| endpoint.clone())
            .collect::<Vec<_>>()
    }

    pub fn all_endpoints(&self) -> Vec<&Endpoint> {
        self.endpoints
            .iter()
            .filter(|(endpoint, _healthiness)| endpoint.weight != 0)
            .map(|(endpoint, _)| endpoint)
            .collect::<Vec<_>>()
    }

    // pub fn select_endpoint(&self, ctx: &GatewayContext) -> Option<String> {
    //     let mut available_endpoints = self.healthy_endpoints();
    //     if available_endpoints.is_empty() {
    //         available_endpoints = self.all_endpoints();
    //     }

    //     let context = Context {
    //         remote_addr: &ctx.remote_addr,
    //         upstream_addrs: &available_endpoints,
    //     };

    //     let endpoint = self.client.strategy.select_endpoint(&context).to_string();

    //     Some(endpoint)
    // }
}

