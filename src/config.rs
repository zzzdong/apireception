use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    iter::FromIterator,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::SystemTime,
};

use arc_swap::ArcSwap;
use drain::Watch;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Notify;
use tokio_rustls::{rustls::sign::CertifiedKey, webpki::DnsName};

use crate::error::{unsupport_file, upstream_not_found, ConfigError};
use crate::upstream::{Upstream, UpstreamMap};
use crate::{
    health::HealthConfig,
    router::{PathRouter, Route},
};

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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Registry {
    #[serde(default)]
    pub routes: Vec<RouteConfig>,
    #[serde(default)]
    pub upstreams: Vec<UpstreamConfig>,
}


#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum RegistryProvider {
    #[serde(rename="etcd")]
    Etcd(EtcdProvider),
    #[serde(rename="file")]
    File(FileProvider),
}

impl RegistryProvider {
    fn load_registry(&self) -> Result<Registry, ConfigError> {
        // TODO
        match self {
            RegistryProvider::Etcd(cfg) => {
                unimplemented!()
            }
            RegistryProvider::File(cfg) => {
                Registry::load_file(&cfg.path)
            }
        }
    }
}

impl Default for RegistryProvider {
    fn default() -> Self {
        RegistryProvider::File(FileProvider{
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

impl Registry {
    pub fn build_router(&self) -> Result<PathRouter, ConfigError> {
        let mut router = PathRouter::new();

        let upstream_set: HashSet<&str> =
            HashSet::from_iter(self.upstreams.iter().map(|up| up.id.as_str()));

        for r in &self.routes {
            upstream_set
                .get(r.upstream_id.as_str())
                .ok_or_else(|| upstream_not_found(&r.upstream_id))?;

            let route = Route::new(r)?;

            for uri in &r.uris {
                let endpoint = router.at_or_default(uri);
                endpoint.push(route.clone());
                endpoint.sort_unstable_by_key(|r| Reverse(r.priority))
            }
        }

        Ok(router)
    }

    pub fn build_upstream_map(&self) -> Result<UpstreamMap, ConfigError> {
        let mut upstreams: UpstreamMap = HashMap::new();

        for u in &self.upstreams {
            let upstream = Upstream::new(u)?;
            upstreams.insert(u.name.clone(), Arc::new(RwLock::new(upstream)));
        }

        Ok(upstreams)
    }

    // pub async fn load_db(&mut self, db: Database) -> Result<(), ConfigError> {
    //     // load routes
    //     let routes_col = db.collection::<RouteConfig>(COL_ROUTES);

    //     let cursor = routes_col.find(None, None).await?;

    //     let routes: Vec<RouteConfig> = cursor.try_collect().await?;

    //     self.routes = routes;

    //     // load upstreams
    //     let upstreams_col = db.collection::<UpstreamConfig>(COL_UPSTREAMS);

    //     let cursor = upstreams_col.find(None, None).await?;

    //     let upstreams: Vec<UpstreamConfig> = cursor.try_collect().await?;

    //     self.upstreams = upstreams;

    //     Ok(())
    // }

    // pub async fn dump_db(&mut self, db: Database) -> Result<(), ConfigError> {
    //     // insert routes
    //     let routes_col = db.collection::<RouteConfig>(COL_ROUTES);

    //     let _ret = routes_col.insert_many(self.routes.clone(), None).await?;

    //     // insert upstreams
    //     let upstreams_col = db.collection::<UpstreamConfig>(COL_UPSTREAMS);

    //     let _ret = upstreams_col
    //         .insert_many(self.upstreams.clone(), None)
    //         .await?;

    //     Ok(())
    // }

    pub fn load_file(path: impl AsRef<Path>) -> Result<Registry, ConfigError> {
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

    pub fn dump_file(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
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

#[derive(Clone)]
pub struct RuntimeConfig {
    pub http_addr: SocketAddr,
    pub https_addr: SocketAddr,
    pub adminapi_addr: Option<SocketAddr>,
    pub certificates: Arc<HashMap<DnsName, CertifiedKey>>,
    pub shared_data: SharedData,

    pub config_notify: Arc<Notify>,
    pub watch: Watch,

    pub config: Arc<Config>,
    pub registry: Arc<RwLock<Registry>>,
}

impl RuntimeConfig {
    pub async fn new(cfg: Config, watch: Watch) -> Result<Self, ConfigError> {
        let http_addr = cfg.server.http_addr.parse()?;
        let https_addr = cfg.server.https_addr.parse()?;
        let adminapi_addr = if cfg.admin.enable {
            Some(cfg.admin.adminapi_addr.parse::<SocketAddr>()?)
        } else {
            None
        };

        // load registry
        let registry = cfg.registry_provider.load_registry()?;

        let certificates = Arc::new(HashMap::new());
        let shared_data = SharedData::new(&registry)?;
        let config = Arc::new(cfg);
        let config_notify = Arc::new(Notify::new());
        let registry = Arc::new(RwLock::new(registry));

        Ok(RuntimeConfig {
            http_addr,
            https_addr,
            adminapi_addr,
            shared_data,
            certificates,
            config,
            registry,
            config_notify,
            watch,
        })
    }

    pub fn start_watch_config(&self) {
        let registry = self.registry.clone();
        let notify = self.config_notify.clone();
        let shared_data = self.shared_data.clone();

        tokio::spawn(async move {
            loop {
                notify.notified().await;

                Self::apply_config(registry.clone(), shared_data.clone());
            }
        });
    }

    pub fn apply_config(registry: Arc<RwLock<Registry>>, shared_data: SharedData) {
        let registry = registry.read().unwrap();
        match shared_data.reload(&registry) {
            Ok(_) => {
                let mut path = std::env::temp_dir();
                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap();
                let filename = format!("apireception-config-{:?}.yaml", now.as_secs_f32());

                path.push(filename);

                registry.dump_file(path).unwrap();
            }
            Err(err) => {
                tracing::error!(%err, "apply config failed")
            }
        }
    }
}

#[derive(Clone)]
pub struct SharedData {
    pub router: Arc<ArcSwap<PathRouter>>,
    pub upstreams: Arc<ArcSwap<UpstreamMap>>,
}

impl SharedData {
    pub fn new(registry: &Registry) -> Result<Self, ConfigError> {
        let router = registry.build_router()?;
        let upstreams = registry.build_upstream_map()?;

        Ok(SharedData {
            router: Arc::new(ArcSwap::new(Arc::new(router))),
            upstreams: Arc::new(ArcSwap::new(Arc::new(upstreams))),
        })
    }

    pub fn reload(&self, registry: &Registry) -> Result<(), ConfigError> {
        let router = registry.build_router()?;
        let upstreams = registry.build_upstream_map()?;

        self.router.store(Arc::new(router));
        self.upstreams.store(Arc::new(upstreams));

        Ok(())
    }
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

        dump_file(&cfg, "config/config.yaml").unwrap();

        let registry = Registry {
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

        dump_file(&registry, "config/apireception.yaml").unwrap();
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
