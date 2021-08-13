use std::{
    cmp::Reverse,
    collections::HashMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_rustls::{rustls::sign::CertifiedKey, webpki::DNSName};

use crate::error::{unsupport_file, ConfigError};
use crate::upstream::Upstream;
use crate::{
    health::HealthConfig,
    router::{PathRouter, Route},
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
    pub plugins: HashMap<String, PluginConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginConfig {
    pub enable: bool,
    #[serde(flatten)]
    pub config: Value,
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
    pub config: Config,
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
                let endpoint = router.at_or_default(uri);
                endpoint.push(route.clone());
                endpoint.sort_unstable_by_key(|r| Reverse(r.priority))
            }
        }

        Ok(SharedData {
            router,
            upstreams,
            config: cfg.clone(),
        })
    }
}

#[cfg(test)]
mod test {
    use crate::plugins::{PathRewriteConfig, TrafficSplitConfig, TrafficSplitRule};

    use super::*;

    #[test]
    fn plugin_config() {
        #[derive(Debug, Clone, Default, Deserialize, Serialize)]
        pub struct Plugins {
            pub plugins: HashMap<String, PluginConfig>,
        }

        let s = r#"{
                "plugins": {
                    "path_rewrite": {
                        "enable": true,
                        "Static": "/hello"
                    },
                    "traffic_split": {
                        "enable": true,
                        "rules": [
                            {
                                "matcher": "",
                                "upstream_id": "upstream_id-001"
                            }
                        ]
                    }
                }
            }"#;

        let value: Plugins = serde_json::from_str(s).unwrap();

        println!("ret={:?}", value);

        for (k, v) in value.plugins {
            match k.as_str() {
                "path_rewrite" => {
                    let cfg: PathRewriteConfig = serde_json::from_value(v.config).unwrap();

                    println!("path_rewrite cfg={:?}", cfg);
                }
                "traffic_split" => {
                    let cfg: TrafficSplitConfig = serde_json::from_value(v.config).unwrap();

                    println!("traffic_split cfg={:?}", cfg);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn example_config() {
        let mut plugins = HashMap::new();

        let path_rewrite =
            PathRewriteConfig::RegexReplace(String::from("/hello/(.*)"), String::from("/$1"));

        let traffic_split = TrafficSplitConfig {
            rules: vec![TrafficSplitRule {
                matcher: r#"PathRegexp('/hello/world/\(.*\)')"#.to_string(),
                upstream_id: "hello-to-tom".to_string(),
            }],
        };

        plugins.insert(
            "path_rewrite".to_string(),
            PluginConfig {
                enable: true,
                config: serde_json::to_value(path_rewrite).unwrap(),
            },
        );

        plugins.insert(
            "traffic_split".to_string(),
            PluginConfig {
                enable: true,
                config: serde_json::to_value(traffic_split).unwrap(),
            },
        );

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
                    uris: vec!["/hello".to_string()],
                    upstream_id: "upstream-001".to_string(),
                    matcher: "".to_string(),
                    priority: 0,
                    plugins: HashMap::new(),
                    ..Default::default()
                },
                RouteConfig {
                    id: "hello-to-tom".to_string(),
                    name: "hello-to-tom".to_string(),
                    uris: vec!["/hello/*".to_string()],
                    upstream_id: "upstream-002".to_string(),
                    matcher: "Query('name', 'tom')".to_string(),
                    priority: 100,
                    plugins: plugins,
                    ..Default::default()
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
