use std::{collections::HashMap, net::SocketAddr, sync::RwLock};

use hyper::Uri;
use rand::{thread_rng, Rng};

use crate::{config::Endpoint, http::HyperRequest};

pub struct Context<'a> {
    pub remote_addr: &'a SocketAddr,
    pub upstream_addrs: &'a [Endpoint],
    pub req: &'a HyperRequest,
}

pub trait LoadBalanceStrategy: Send + Sync + std::fmt::Debug {
    fn select_endpoint<'a>(&self, context: &'a Context) -> &'a str;
    fn on_send_request(&self, uri: &Uri) {
        let _ = uri;
    }
    fn on_request_done(&self, uri: &Uri) {
        let _ = uri;
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
    fn select_endpoint<'a>(&self, context: &'a Context) -> &'a str {
        let index = thread_rng().gen_range(0..context.upstream_addrs.len());

        &context.upstream_addrs[index].addr
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
    fn select_endpoint<'a>(&self, context: &'a Context) -> &'a str {
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
pub struct LeastRequest {
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
    fn select_endpoint<'a>(&self, context: &'a Context) -> &'a str {
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
    use crate::config::Endpoint;

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

        let req = HyperRequest::new("".into());
        let endpoints = endpoints.iter().map(Clone::clone).collect::<Vec<_>>();

        let ctx = Context {
            remote_addr: &"127.0.0.1:80".parse::<SocketAddr>().unwrap(),
            upstream_addrs: &endpoints[..],
            req: &req,
        };

        let weighted = WeightedRandom::new();

        let mut result: HashMap<&str, u32> = HashMap::new();
        for _ in 0..100000 {
            let got = weighted.select_endpoint(&ctx);

            result.entry(got).and_modify(|sum| *sum += 1).or_default();
        }

        println!("weighted ret= {:?}", result);

        let random = Random::new();

        let mut result: HashMap<&str, u32> = HashMap::new();
        for _ in 0..1000 {
            let got = random.select_endpoint(&ctx);

            result.entry(got).and_modify(|sum| *sum += 1).or_default();
        }

        println!("random ret= {:?}", result);
    }
}
