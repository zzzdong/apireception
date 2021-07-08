use std::{
    cmp::Reverse,
    collections::HashMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};
use tokio_rustls::{rustls::sign::CertifiedKey, webpki::DNSName};

use crate::{health::HealthConfig, router::{PathRouter, Route}};
use crate::upstream::Upstream;
use crate::{
    error::{unsupport_file, ConfigError},
    plugins::PluginItem,
};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Config {
    pub server: ServerConfig,
    pub routes: Vec<RouteConfig>,
    pub upstreams: Vec<UpstreamConfig>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ServerConfig {
    pub log_level: String,
    pub http_addr: String,
    pub https_addr: String,
    pub tls_config: HashMap<String, TlsConfig>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RouteConfig {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub uris: Vec<String>,
    pub upstream_id: String,
    #[serde(default)]
    pub matcher: String,
    #[serde(default)]
    pub priority: u32,
    #[serde(default)]
    pub plugins: Vec<PluginItem>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct UpstreamConfig {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub endpoints: Vec<Endpoint>,
    pub strategy: String,
    #[serde(default)]
    pub is_https: bool,
    pub health_check: HealthConfig,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Endpoint {
    pub addr: String,
    pub weight: u32,
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Config, ConfigError> {
        let path = path.as_ref();
        let ext = path
            .extension()
            .and_then(|p| p.to_str())
            .ok_or_else(unsupport_file)?;

        let content = std::fs::read_to_string(path)?;

        tracing::info!(?content, "file ok");

        let cfg = match ext {
            "yaml" => serde_yaml::from_str(&content)?,
            "json" => serde_json::from_str(&content)?,
            "toml" => toml::from_str(&content)?,
            _ => {
                return Err(unsupport_file().into());
            }
        };

        Ok(cfg)
    }

    pub fn dumps(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
        let path = path.as_ref();
        let ext = path
            .extension()
            .and_then(|p| p.to_str())
            .ok_or_else(unsupport_file)?;

        let contents = match ext {
            "yaml" => serde_yaml::to_string(self)?,
            "json" => serde_json::to_string_pretty(self)?,
            "toml" => toml::to_string_pretty(self)?,
            _ => {
                return Err(unsupport_file().into());
            }
        };

        std::fs::write(path, contents)?;
        Ok(())
    }
}

pub struct RuntimeConfig {
    pub http_addr: SocketAddr,
    pub https_addr: SocketAddr,
    pub certificates: Arc<HashMap<DNSName, CertifiedKey>>,
    pub shared_data: Arc<ArcSwap<SharedData>>,
}

impl RuntimeConfig {
    pub fn new(cfg: &Config) -> Result<Self, ConfigError> {
        let http_addr = cfg.server.http_addr.parse()?;
        let https_addr = cfg.server.https_addr.parse()?;
        let certificates = Arc::new(HashMap::new());
        let shared_data = Arc::new(ArcSwap::from_pointee(SharedData::new(cfg)?));

        Ok(RuntimeConfig {
            http_addr,
            https_addr,
            shared_data,
            certificates,
        })
    }
}

pub struct SharedData {
    pub router: PathRouter,
    pub upstreams: HashMap<String, Arc<RwLock<Upstream>>>,
}

impl SharedData {
    pub fn new(cfg: &Config) -> Result<Self, ConfigError> {
        let mut upstreams: HashMap<String, Arc<RwLock<Upstream>>> = HashMap::new();

        for u in &cfg.upstreams {
            let upstream = Upstream::new(u)?;
            upstreams.insert(u.name.clone(), Arc::new(RwLock::new(upstream)));
        }

        let mut router = PathRouter::new();

        for r in &cfg.routes {
            let route = Route::new(r, upstreams.clone())?;

            for uri in &r.uris {
                router.add_or_update_with(uri, vec![route.clone()], |routes| {
                    routes.push(route.clone());
                    routes.sort_unstable_by_key(|r| Reverse(r.priority))
                });
            }
        }

        Ok(SharedData { router, upstreams })
    }
}

#[cfg(test)]
mod test {
    use crate::plugins::{PathRewriteConfig, TrafficSplitConfig, TrafficSplitRule};

    use super::*;

    #[test]
    fn example_config() {
        let cfg = Config {
            server: ServerConfig {
                log_level: "debug".to_string(),
                http_addr: "0.0.0.0:8080".to_string(),
                https_addr: "0.0.0.0:8443".to_string(),
                tls_config: [(
                    "www.example.com".to_string(),
                    TlsConfig {
                        cert_path: PathBuf::from("example.cert"),
                        key_path: PathBuf::from("example.key"),
                    },
                )]
                .iter()
                .cloned()
                .collect(),
            },

            routes: vec![
                RouteConfig {
                    id: "hello".to_string(),
                    name: "hello".to_string(),
                    desc: String::new(),
                    uris: vec!["/hello".to_string()],
                    upstream_id: "upstream-001".to_string(),
                    matcher: "".to_string(),
                    priority: 0,
                    plugins: vec![],
                },
                RouteConfig {
                    id: "hello-to-tom".to_string(),
                    name: "hello-to-tom".to_string(),
                    desc: String::new(),
                    uris: vec!["/hello/*".to_string()],
                    upstream_id: "upstream-002".to_string(),
                    matcher: "Query('name', 'tom')".to_string(),
                    priority: 100,
                    plugins: vec![
                        PluginItem::PathRewrite(PathRewriteConfig::RegexReplace(
                            String::from("/hello/(.*)"),
                            String::from("/$1"),
                        )),
                        PluginItem::TrafficSplit(TrafficSplitConfig {
                            enable: true,
                            rules: vec![TrafficSplitRule {
                                matcher: r#"PathRegexp('/hello/world/\(.*\)')"#.to_string(),
                                upstream_id: "hello-to-tom".to_string(),
                            }],
                        }),
                    ],
                },
            ],
            upstreams: vec![
                UpstreamConfig {
                    id: "upstream-001".to_string(),
                    name: "upstream-001".to_string(),
                    desc: String::new(),
                    endpoints: vec![Endpoint {
                        addr: "127.0.0.1:5000".to_string(),
                        weight: 1,
                    }],
                    strategy: "random".to_string(),
                    is_https: false,
                    health_check: HealthConfig::default(),
                },
                UpstreamConfig {
                    id: "upstream-002".to_string(),
                    name: "upstream-002".to_string(),
                    desc: String::new(),
                    endpoints: vec![Endpoint {
                        addr: "127.0.0.1:5000".to_string(),
                        weight: 1,
                    }],
                    strategy: "weighted".to_string(),
                    is_https: false,
                    health_check: HealthConfig::default(),
                },
            ],
        };

        cfg.dumps("config.yaml").unwrap();
    }
}
