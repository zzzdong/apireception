use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use arc_swap::ArcSwap;
use hyper::{Body, Request, Uri};
use rand::{thread_rng, Rng};

use crate::config::{Config, Endpoint, UpstreamConfig};
use crate::health::Healthiness;
use crate::http::HyperRequest;

pub struct Upstream {
    pub name: String,
    endpoints: Vec<(Endpoint, ArcSwap<Healthiness>)>,
    strategy: Box<dyn LoadBalanceStrategy + Send + Sync + 'static>,
}

impl Upstream {
    pub fn new(config: &UpstreamConfig) -> Self {
        let endpoints = config
            .endpoints
            .iter()
            .map(|ep| (ep.clone(), ArcSwap::new(Arc::new(Healthiness::Healthly))))
            .collect();

        let strategy = Box::new(Random::new());

        Upstream {
            name: config.name.clone(),
            endpoints,
            strategy,
        }
    }

    pub fn heathy_endpoints<'a>(&'a self) -> Vec<&'a str> {
        self.endpoints
            .iter()
            .filter(|(_, healthiness)| healthiness.load().as_ref() == &Healthiness::Healthly)
            .map(|(ep, _)| ep.addr.as_str())
            .collect::<Vec<_>>()
    }

    pub fn select_upstream<'a>(&'a self, ctx: &'a Context, req: &HyperRequest) -> Uri {
        let endpoint = self.strategy.select_upstream(ctx);

        let path = req.uri().path_and_query().unwrap().clone();
        Uri::builder()
            .scheme("http")
            .authority(endpoint)
            .path_and_query(path)
            .build()
            .unwrap()
    }
}

pub struct Context<'a> {
    // pub client_addr: &'a SocketAddr,
    pub upstream_addrs: &'a [&'a str],
}

trait LoadBalanceStrategy {
    fn select_upstream<'a>(&self, context: &'a Context) -> &'a str;
    fn on_tcp_open(&mut self, endpoint: &str) {}
    fn on_tcp_close(&mut self, endpoint: &str) {}
}

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

    fn on_tcp_open(&mut self, endpoint: &str) {
        let mut connections = self.connections.write().unwrap();
        *connections.entry(endpoint.to_string()).or_insert(0) += 1;
    }

    fn on_tcp_close(&mut self, endpoint: &str) {
        let mut connections = self.connections.write().unwrap();
        *connections.entry(endpoint.to_string()).or_insert(0) -= 1;
    }
}
