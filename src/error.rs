pub type Result<T> = std::result::Result<T, crate::error::Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error")]
    Io(#[from] std::io::Error),
    #[error("http error")]
    Http(#[from] hyper::Error),
    #[error("config error")]
    Config(#[from] ConfigError),
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("yaml config error")]
    Yaml(#[from] serde_yaml::Error),
    #[error("json config error")]
    Json(#[from] serde_json::Error),
    #[error("toml encode error")]
    TomlEncode(#[from] toml::ser::Error),
    #[error("toml decode error")]
    TomlDecode(#[from] toml::de::Error),
    #[error("io error")]
    Io(#[from] std::io::Error),
    #[error("parse addr error")]
    AddrParse(#[from] std::net::AddrParseError),
    #[error("parse uri error")]
    UriParse(#[from] hyper::http::uri::InvalidUri),
    #[error("parse match error")]
    MatcherParse(#[from] MatcherParseError),
    #[error("etcd client error")]
    EtcdClient(#[from] etcdv3client::Error),
    #[error("{0}")]
    Message(String),
    #[error("upstream<{0}> not found")]
    UpstreamNotFound(String),
    #[error("unknown strategy<{0}>")]
    UnknownLBStrategy(String),
}

#[derive(Debug, PartialEq)]
pub struct MatcherParseError(String);

impl MatcherParseError {
    pub fn new(e: String) -> Self {
        MatcherParseError(e)
    }
}

impl std::fmt::Display for MatcherParseError {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt.write_str(&self.0)
    }
}

impl std::error::Error for MatcherParseError {
    fn description(&self) -> &str {
        &self.0
    }
}

pub fn upstream_not_found(upstream: impl ToString) -> ConfigError {
    ConfigError::UpstreamNotFound(upstream.to_string())
}

pub fn unsupport_file() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Unsupported, "file format not support")
}
