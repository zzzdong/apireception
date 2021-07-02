use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use arc_swap::ArcSwap;
use hyper::http::uri::{Scheme, Uri};
use rand::{thread_rng, Rng};

use crate::config::{Endpoint, UpstreamConfig};
use crate::error::ConfigError;
use crate::health::Healthiness;
use crate::http_client::GatewayClient;

pub struct Upstream {
    pub name: String,
    pub scheme: Scheme,
    pub client: GatewayClient,
    endpoints: Vec<(Endpoint, ArcSwap<Healthiness>)>,
}

impl Upstream {
    pub fn new(config: &UpstreamConfig) -> Result<Self, ConfigError> {
        let endpoints = config
            .endpoints
            .iter()
            .map(|ep| (ep.clone(), ArcSwap::new(Arc::new(Healthiness::Healthly))))
            .collect();

        let scheme = if config.is_https {
            Scheme::HTTPS
        } else {
            Scheme::HTTP
        };

        let strategy: Arc<Box<dyn LoadBalanceStrategy>> = Arc::new(Box::new(Random::new()));

        let client = GatewayClient::new(strategy);

        Ok(Upstream {
            name: config.name.clone(),
            endpoints,
            scheme,
            client,
        })
    }

    pub fn heathy_endpoints(&self) -> Vec<&str> {
        self.endpoints
            .iter()
            .filter(|(_, healthiness)| healthiness.load().as_ref() == &Healthiness::Healthly)
            .map(|(ep, _)| ep.addr.as_str())
            .collect::<Vec<_>>()
    }

    pub fn select_upstream<'a>(&'a self, ctx: &'a Context) -> String {
        self.client.strategy.select_upstream(ctx).to_string()
    }
}

pub struct Context<'a> {
    pub remote_addr: &'a SocketAddr,
    pub upstream_addrs: &'a [&'a str],
}

pub trait LoadBalanceStrategy: Send + Sync + std::fmt::Debug {
    fn select_upstream<'a>(&self, context: &'a Context) -> &'a str;
    fn on_send_request(&self, uri: &Uri) {}
    fn on_request_done(&self, uri: &Uri) {}
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

        context.upstream_addrs[index]
    }
}

#[derive(Debug)]
struct LeastConnection {
    connections: RwLock<HashMap<String, usize>>,
}

impl LeastConnection {
    pub fn new() -> Self {
        LeastConnection {
            connections: RwLock::new(HashMap::new()),
        }
    }
}

impl LoadBalanceStrategy for LeastConnection {
    fn select_upstream<'a>(&self, context: &'a Context) -> &'a str {
        let connections = self.connections.read().unwrap();

        let address_indices: Vec<usize> =
            if connections.len() == 0 || context.upstream_addrs.len() > connections.len() {
                // if no TCP connections have been opened yet, or some backend servers are not used yet, we'll use them for the next request
                context
                    .upstream_addrs
                    .iter()
                    .enumerate()
                    .filter(|(_, address)| !connections.contains_key(**address))
                    .map(|(index, _)| index)
                    .collect()
            } else {
                let backend_address_map = context
                    .upstream_addrs
                    .iter()
                    .enumerate()
                    .map(|(index, address)| (*address, index))
                    .collect::<HashMap<_, _>>();
                let mut least_connections = connections.iter().collect::<Vec<_>>();

                least_connections.sort_by(|a, b| a.1.cmp(b.1));

                let min_connection_count = least_connections[0].1;
                least_connections
                    .iter()
                    .take_while(|(_, connection_count)| *connection_count == min_connection_count)
                    .map(|tuple| tuple.0)
                    .map(|address| *backend_address_map.get(address.as_str()).unwrap())
                    .collect()
            };

        if address_indices.len() == 1 {
            context.upstream_addrs[address_indices[0]]
        } else {
            let index = thread_rng().gen_range(0..address_indices.len());

            context.upstream_addrs[address_indices[index]]
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
