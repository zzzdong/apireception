
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Config {
    pub server: Server,
    pub routes: Vec<Route>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Server {
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
pub struct Route {
    pub name: String,
    pub uris: Vec<String>,
    pub matcher: String,
    pub upstream: Upstream,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Upstream {
    pub name: String,
    pub endpoits: Vec<Endpoint>,
    pub strategy: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Endpoint {
    pub addr: String,
    pub weight: u32,
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Config> {
        let path = path.as_ref().clone();
        let ext = path
            .extension()
            .and_then(|p| p.to_str())
            .ok_or(unsupport_file())?;

        let content = std::fs::read_to_string(path)?;

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

    pub fn dumps(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let s = serde_json::to_string(self)?;

        let path = path.as_ref().clone();
        let ext = path
            .extension()
            .and_then(|p| p.to_str())
            .ok_or(unsupport_file())?;

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

fn unsupport_file() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Unsupported, "file format not support")
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn example_config() {
        let cfg = Config {
            server: Server {
                log_level: "debug".to_string(),
                http_addr: "0.0.0.0:80".to_string(),
                https_addr: "0.0.0.0:443".to_string(),
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
                Route {
                    name: "hello".to_string(),
                    uris: vec!["/hello".to_string()],
                    upstream: Upstream {
                        name: "upstream-001".to_string(),
                        endpoits: vec![Endpoint {
                            addr: "127.0.0.1:8080".to_string(),
                            weight: 1,
                        }],
                        strategy: "random".to_string(),
                    },
                    matcher: "Path('/hello')".to_string(),
                },
                Route {
                    name: "world".to_string(),
                    uris: vec!["/world".to_string()],
                    upstream: Upstream {
                        name: "upstream-002".to_string(),
                        endpoits: vec![Endpoint {
                            addr: "127.0.0.1:8090".to_string(),
                            weight: 1,
                        }],
                        strategy: "random".to_string(),
                    },
                    matcher: "Path('/world')".to_string(),
                },
            ],
        };

        cfg.dumps("config.yaml").unwrap();
    }
}
