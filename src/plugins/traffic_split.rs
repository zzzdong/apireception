use serde::{Deserialize, Serialize};

use crate::{error::ConfigError, http::HyperRequest, matcher::RouteMatcher};

use super::Plugin;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TrafficSplitConfig {
    pub rules: Vec<TrafficSplitRule>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TrafficSplitRule {
    pub matcher: String,
    pub upstream_id: String,
}

pub(crate) struct TrafficSplitPlugin {
    rules: Vec<TrafficSplitItem>,
}

pub(crate) struct TrafficSplitItem {
    matcher: RouteMatcher,
    upstream_id: String,
}

impl TrafficSplitItem {
    pub fn new(cfg: &TrafficSplitRule) -> Result<Self, ConfigError> {
        let matcher = RouteMatcher::parse(&cfg.matcher)?;

        Ok(TrafficSplitItem {
            matcher,
            upstream_id: cfg.upstream_id.to_string(),
        })
    }
}

impl TrafficSplitPlugin {
    pub fn new(cfg: TrafficSplitConfig) -> Result<Self, ConfigError> {
        let mut rules = Vec::new();

        for rule in &cfg.rules {
            rules.push(TrafficSplitItem::new(rule)?);
        }

        Ok(TrafficSplitPlugin { rules })
    }

    fn select_upstream(&self, req: &HyperRequest) -> Option<String> {
        for rule in &self.rules {
            if rule.matcher.matchs(req) {
                return Some(rule.upstream_id.clone());
            }
        }
        None
    }
}

impl Plugin for TrafficSplitPlugin {
    fn name(&self) -> &str {
        "trafic_split"
    }

    fn priority(&self) -> u32 {
        1001
    }

    fn on_access(
        &self,
        ctx: &mut crate::context::GatewayContext,
        req: crate::http::HyperRequest,
    ) -> Result<crate::http::HyperRequest, crate::http::HyperResponse> {
        ctx.upstream_id = self.select_upstream(&req);

        Ok(req)
    }
}
