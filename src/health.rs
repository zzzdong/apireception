use std::time::Duration;

use hyper::StatusCode;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct HealthConfig {
    pub slow_threshold: i64,
    pub timeout: u64,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Healthiness {
    Healthly,
    Slow(Duration),
    Unresponsive(Option<StatusCode>),
}
