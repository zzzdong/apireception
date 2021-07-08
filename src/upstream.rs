use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use arc_swap::ArcSwap;
use hyper::http::uri::{Scheme, Uri};
use rand::{thread_rng, Rng};

use crate::config::{Endpoint, UpstreamConfig};
use crate::context::GatewayContext;
use crate::error::ConfigError;
use crate::health::{HealthConfig, Healthiness};
use crate::http_client::GatewayClient;

pub struct Upstream {
    pub name: String,
    pub scheme: Scheme,
    pub client: GatewayClient,
    pub endpoints: Vec<(Endpoint, ArcSwap<Healthiness>)>,
    pub health_config: HealthConfig,
}

impl Upstream {
    pub fn new(cfg: &UpstreamConfig) -> Result<Self, ConfigError> {
        let endpoints = cfg
            .endpoints
            .iter()
            .map(|ep| (ep.clone(), ArcSwap::new(Arc::new(Healthiness::Healthly))))
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

    pub fn heathy_endpoints(&self) -> Vec<&Endpoint> {
        self.endpoints
            .iter()
            .filter(|(endpoint, healthiness)| {
                (endpoint.weight != 0) && (healthiness.load().as_ref() == &Healthiness::Healthly)
            })
            .map(|(endpoint, _)| endpoint)
            .collect::<Vec<_>>()
    }

    pub fn select_upstream(&self, ctx: &GatewayContext) -> Option<String> {
        let available_endpoints = &self.heathy_endpoints();
        if available_endpoints.is_empty() {
            return None;
        }

        let context = Context {
            remote_addr: &ctx.remote_addr,
            upstream_addrs: available_endpoints,
        };

        let endpoint = self.client.strategy.select_upstream(&context).to_string();

        Some(endpoint)
    }
}

pub struct Context<'a> {
    pub remote_addr: &'a SocketAddr,
    pub upstream_addrs: &'a [&'a Endpoint],
}

pub trait LoadBalanceStrategy: Send + Sync + std::fmt::Debug {
    fn select_upstream<'a>(&self, context: &'a Context) -> &'a str;
    fn on_send_request(&self, uri: &Uri) {
        let _ = uri;
    }
    fn on_request_done(&self, uri: &Uri) {
        let _ = uri;
    }
}

#[derive(Debug)]
struct Random {}

impl Random {
    pub fn new() -> Self {
        Random {}
    }
}

impl LoadBalanceStrategy for Random {
    fn select_upstream<'a>(&self, context: &'a Context) -> &'a str {
        let index = thread_rng().gen_range(0..context.upstream_addrs.len());

        &context.upstream_addrs[index].addr
    }
}

#[derive(Debug)]
struct WeightedRandom {}

impl WeightedRandom {
    pub fn new() -> Self {
        WeightedRandom {}
    }
}

impl LoadBalanceStrategy for WeightedRandom {
    fn select_upstream<'a>(&self, context: &'a Context) -> &'a str {
        let total_weigth = context
            .upstream_addrs
            .iter()
            .fold(0, |sum, a| sum + a.weight);

        let random = thread_rng().gen_range(0..total_weigth);

        let mut curr = 0;
        for ep in context.upstream_addrs {
            curr += ep.weight;
            if random < curr {
                return &ep.addr;
            }
        }

        unreachable!()
    }
}

#[derive(Debug)]
struct LeastRequest {
    connections: RwLock<HashMap<String, usize>>,
}

impl LeastRequest {
    pub fn new() -> Self {
        LeastRequest {
            connections: RwLock::new(HashMap::new()),
        }
    }
}

impl LoadBalanceStrategy for LeastRequest {
    fn select_upstream<'a>(&self, context: &'a Context) -> &'a str {
        let connections = self.connections.read().unwrap();

        let address_indices: Vec<usize> =
            if connections.len() == 0 || context.upstream_addrs.len() > connections.len() {
                // if some upstream servers are not used yet, we'll use them for the next request
                context
                    .upstream_addrs
                    .iter()
                    .enumerate()
                    .filter(|(_, endpoint)| !connections.contains_key(&endpoint.addr))
                    .map(|(index, _)| index)
                    .collect()
            } else {
                let upstream_addr_map = context
                    .upstream_addrs
                    .iter()
                    .enumerate()
                    .map(|(index, endpoint)| (&endpoint.addr, index))
                    .collect::<HashMap<_, _>>();
                let mut least_connections = connections.iter().collect::<Vec<_>>();

                least_connections.sort_unstable_by_key(|key| key.1);

                let min_connection_count = least_connections[0].1;
                least_connections
                    .iter()
                    .take_while(|(_, connection_count)| *connection_count == min_connection_count)
                    .map(|tuple| tuple.0)
                    .map(|address| upstream_addr_map.get(address).unwrap())
                    .cloned()
                    .collect()
            };

        if address_indices.len() == 1 {
            &context.upstream_addrs[address_indices[0]].addr
        } else {
            let index = thread_rng().gen_range(0..address_indices.len());

            &context.upstream_addrs[address_indices[index]].addr
        }
    }

    fn on_send_request(&self, endpoint: &Uri) {
        let mut connections = self.connections.write().unwrap();
        *connections
            .entry(endpoint.authority().unwrap().to_string())
            .or_insert(0) += 1;
    }

    fn on_request_done(&self, endpoint: &Uri) {
        let mut connections = self.connections.write().unwrap();
        *connections
            .entry(endpoint.authority().unwrap().to_string())
            .or_insert(0) -= 1;
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_weighted_random() {
        let endpoints = vec![
            Endpoint {
                addr: String::from("aaa"),
                weight: 10,
            },
            Endpoint {
                addr: String::from("bbb"),
                weight: 10,
            },
            Endpoint {
                addr: String::from("ccc"),
                weight: 80,
            },
        ];

        let endpoints = endpoints.iter().collect::<Vec<_>>();

        let ctx = Context {
            remote_addr: &"127.0.0.1:80".parse::<SocketAddr>().unwrap(),
            upstream_addrs: &endpoints[..],
        };

        let weighted = WeightedRandom::new();

        let mut result: HashMap<&str, u32> = HashMap::new();
        for _ in 0..100000 {
            let got = weighted.select_upstream(&ctx);

            result.entry(got).and_modify(|sum| *sum += 1).or_default();
        }

        println!("weighted ret= {:?}", result);

        let random = Random::new();

        let mut result: HashMap<&str, u32> = HashMap::new();
        for _ in 0..1000 {
            let got = random.select_upstream(&ctx);

            result.entry(got).and_modify(|sum| *sum += 1).or_default();
        }

        println!("random ret= {:?}", result);
    }
}
