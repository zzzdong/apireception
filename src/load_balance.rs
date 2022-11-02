use std::{collections::HashMap, sync::RwLock};

use hyper::Uri;
use rand::{thread_rng, Rng};

use crate::{context::GatewayContext, http::HyperRequest};

pub trait LoadBalanceStrategy: Send + Sync + std::fmt::Debug {
    fn select_endpoint<'a>(&self, ctx: &'a GatewayContext, req: &HyperRequest) -> &'a Uri;
    fn on_send_request(&self, ctx: &GatewayContext, endpoint: &Uri) {
        let _ = endpoint;
    }
    fn on_request_done(&self, ctx: &GatewayContext, endpoint: &Uri) {
        let _ = endpoint;
    }
}

#[derive(Debug)]
pub struct Random {}

impl Random {
    pub fn new() -> Self {
        Random {}
    }
}

impl LoadBalanceStrategy for Random {
    fn select_endpoint<'a>(&self, ctx: &'a GatewayContext, req: &HyperRequest) -> &'a Uri {
        let index = thread_rng().gen_range(0..ctx.available_endpoints.len());

        &ctx.available_endpoints[index].target
    }
}

#[derive(Debug)]
pub struct WeightedRandom {}

impl WeightedRandom {
    pub fn new() -> Self {
        WeightedRandom {}
    }
}

impl LoadBalanceStrategy for WeightedRandom {
    fn select_endpoint<'a>(&self, ctx: &'a GatewayContext, req: &HyperRequest) -> &'a Uri {
        let total_weigth = ctx
            .available_endpoints
            .iter()
            .fold(0, |sum, a| sum + a.weight);

        let random = thread_rng().gen_range(0..total_weigth);

        let mut curr = 0;
        for ep in &ctx.available_endpoints {
            curr += ep.weight;
            if random < curr {
                return &ep.target;
            }
        }

        unreachable!()
    }
}

#[derive(Debug)]
pub struct LeastRequest {
    connections: RwLock<HashMap<Uri, usize>>,
}

impl LeastRequest {
    pub fn new() -> Self {
        LeastRequest {
            connections: RwLock::new(HashMap::new()),
        }
    }
}

impl LoadBalanceStrategy for LeastRequest {
    fn select_endpoint<'a>(&self, context: &'a GatewayContext, req: &HyperRequest) -> &'a Uri {
        let connections = self.connections.read().unwrap();

        let address_indices: Vec<usize> =
            if connections.len() == 0 || context.available_endpoints.len() > connections.len() {
                // if some upstream servers are not used yet, we'll use them for the next request
                context
                    .available_endpoints
                    .iter()
                    .enumerate()
                    .filter(|(_, endpoint)| !connections.contains_key(&endpoint.target))
                    .map(|(index, _)| index)
                    .collect()
            } else {
                let upstream_addr_map = context
                    .available_endpoints
                    .iter()
                    .enumerate()
                    .map(|(index, endpoint)| (&endpoint.target, index))
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
            &context.available_endpoints[address_indices[0]].target
        } else {
            let index = thread_rng().gen_range(0..address_indices.len());

            &context.available_endpoints[address_indices[index]].target
        }
    }

    fn on_send_request(&self, ctx: &GatewayContext, endpoint: &Uri) {
        let mut connections = self.connections.write().unwrap();
        *connections.entry(endpoint.clone()).or_insert(0) += 1;
    }

    fn on_request_done(&self, ctx: &GatewayContext, endpoint: &Uri) {
        let mut connections = self.connections.write().unwrap();
        *connections.entry(endpoint.clone()).or_insert(0) -= 1;
    }
}

#[cfg(test)]
mod test {
    use hyper::http::uri::Scheme;

    use crate::runtime::Endpoint;

    use super::*;

    #[test]
    fn test_weighted_random() {
        let endpoints = vec![
            Endpoint {
                target: Uri::from_static("http://aaa.com/"),
                weight: 10,
            },
            Endpoint {
                target: Uri::from_static("http://bbb.com/"),
                weight: 10,
            },
            Endpoint {
                target: Uri::from_static("http://ccc.com/"),
                weight: 80,
            },
        ];

        let req = HyperRequest::new("".into());

        let mut ctx = GatewayContext::new(None, Scheme::HTTP, &req);

        let weighted = WeightedRandom::new();

        let mut result: HashMap<&Uri, u32> = HashMap::new();
        for _ in 0..100000 {
            let got = weighted.select_endpoint(&ctx, &req);

            result.entry(got).and_modify(|sum| *sum += 1).or_default();
        }

        println!("weighted ret= {:?}", result);

        let random = Random::new();

        let mut result: HashMap<&Uri, u32> = HashMap::new();
        for _ in 0..1000 {
            let got = random.select_endpoint(&ctx, &req);

            result.entry(got).and_modify(|sum| *sum += 1).or_default();
        }

        println!("random ret= {:?}", result);
    }
}
