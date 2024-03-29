use std::{borrow::Cow, convert::TryFrom};

use hyper::{http::uri::PathAndQuery, Uri};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

use super::Plugin;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathRewriteConfig {
    Keep,
    Static(String),
    RegexReplace(String, String),
}

impl Default for PathRewriteConfig {
    fn default() -> Self {
        PathRewriteConfig::Keep
    }
}

#[derive(Debug, Clone)]
pub(crate) enum PathRewritePlugin {
    Keep,
    Static(String),
    RegexReplace(regex::Regex, String),
}

impl PathRewritePlugin {
    pub fn new(cfg: PathRewriteConfig) -> Result<Self, ConfigError> {
        let path_rewrite = match cfg {
            PathRewriteConfig::Keep => PathRewritePlugin::Keep,
            PathRewriteConfig::Static(ref s) => PathRewritePlugin::Static(s.to_string()),
            PathRewriteConfig::RegexReplace(ref m, ref r) => {
                let re = Regex::new(m).map_err(|e| ConfigError::Message(e.to_string()))?;
                PathRewritePlugin::RegexReplace(re, r.to_string())
            }
        };

        Ok(path_rewrite)
    }

    pub fn path_rewrite<'a>(&self, path: &'a str) -> Cow<'a, str> {
        match self {
            PathRewritePlugin::Keep => Cow::Borrowed(path),
            PathRewritePlugin::Static(ref s) => Cow::Owned(s.to_owned()),
            PathRewritePlugin::RegexReplace(ref re, ref pat) => re.replace(path, pat),
        }
    }
}

impl Plugin for PathRewritePlugin {
    fn name(&self) -> &str {
        "path_rewrite"
    }

    fn priority(&self) -> u32 {
        1002
    }

    fn on_access(
        &self,
        ctx: &mut crate::context::GatewayContext,
        mut req: crate::http::HyperRequest,
    ) -> Result<crate::http::HyperRequest, crate::http::HyperResponse> {
        let _ = ctx;
        let orig_uri = req.uri().clone();

        let path = self.path_rewrite(orig_uri.path()).to_string();

        if path != orig_uri.path() {
            let mut parts = orig_uri.into_parts();

            parts.path_and_query = parts.path_and_query.and_then(|p_and_q| {
                PathAndQuery::try_from(match p_and_q.query() {
                    Some(q) => path + "?" + q,
                    None => path,
                })
                .ok()
            });

            let uri = Uri::from_parts(parts).unwrap();

            *req.uri_mut() = uri;
        }

        Ok(req)
    }
}
