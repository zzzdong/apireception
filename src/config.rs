use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{unsupport_file, ConfigError};
use crate::health::HealthConfig;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(default)]
    pub admin: AdminConfig,
    #[serde(default)]
    pub registry_provider: RegistryProvider,
}

impl Config {
    pub fn load_file(path: impl AsRef<Path>) -> Result<Config, ConfigError> {
        load_file(path)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AdminConfig {
    pub enable: bool,
    pub adminapi_addr: String,
    pub users: Vec<User>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct User {
    pub username: String,
    pub password: String,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum RegistryProvider {
    #[serde(rename = "etcd")]
    Etcd(EtcdProvider),
    #[serde(rename = "file")]
    File(FileProvider),
}

impl Default for RegistryProvider {
    fn default() -> Self {
        RegistryProvider::File(FileProvider {
            path: PathBuf::from("config/apireception.yaml"),
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EtcdProvider {
    pub host: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct FileProvider {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RouteConfig {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub desc: String,
    pub uris: Vec<String>,
    pub upstream_id: String,
    #[serde(default)]
    pub overwrite_host: bool,
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
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub desc: String,
    pub endpoints: Vec<EndpointConfig>,
    pub strategy: String,
    pub health_check: HealthConfig,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EndpointConfig {
    pub addr: String,
    pub weight: u32,
}

pub fn load_file<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> Result<T, ConfigError> {
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

pub fn dump_file<T: serde::Serialize>(data: &T, path: impl AsRef<Path>) -> Result<(), ConfigError> {
    let path = path.as_ref();
    let ext = path
        .extension()
        .and_then(|p| p.to_str())
        .ok_or_else(unsupport_file)?;

    if path.is_file() {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
    }

    let contents = match ext {
        "yaml" => serde_yaml::to_string(data)?,
        "json" => serde_json::to_string_pretty(data)?,
        "toml" => toml::to_string_pretty(data)?,
        _ => {
            return Err(unsupport_file().into());
        }
    };

    std::fs::write(path, contents)?;
    Ok(())
}

#[cfg(test)]
mod test {
    use crate::{
        plugins::{PathRewriteConfig, TrafficSplitConfig, TrafficSplitRule},
        registry::RegistryConfig,
    };

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
            admin: AdminConfig {
                enable: true,
                adminapi_addr: "127.0.0.1:8000".to_string(),
                users: vec![User {
                    username: "admin".to_string(),
                    password: "admin".to_string(),
                }],
            },
            registry_provider: RegistryProvider::default(),
        };

        dump_file(&cfg, "config2/config.yaml").unwrap();

        let registry = RegistryConfig {
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
                    endpoints: vec![EndpointConfig {
                        addr: "127.0.0.1:5000".to_string(),
                        weight: 1,
                    }],
                    strategy: "random".to_string(),

                    health_check: HealthConfig::default(),
                },
                UpstreamConfig {
                    id: "upstream-002".to_string(),
                    name: "upstream-002".to_string(),
                    desc: String::new(),
                    endpoints: vec![EndpointConfig {
                        addr: "127.0.0.1:5000".to_string(),
                        weight: 1,
                    }],
                    strategy: "weighted".to_string(),
                    health_check: HealthConfig::default(),
                },
            ],
        };

        dump_file(&registry, "config2/apireception.yaml").unwrap();
    }

    // #[tokio::test]
    // async fn dump_db() {
    //     let mut cfg = Config::load_file("config.yaml").unwrap();

    //     let db = Client::with_uri_str(&cfg.server.mongo_uri)
    //         .await
    //         .unwrap()
    //         .database(DB_APIRECEPTION);

    //     // TODO: add createIndex
    //     // db.routes.createIndex({"id": 1}, {unique: true})
    //     // db.upstreams.createIndex({"id": 1}, {unique: true})

    //     cfg.dump_db(db).await.unwrap();
    // }
}
