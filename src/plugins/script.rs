use std::convert::TryFrom;

use hyper::{http::uri::PathAndQuery, Uri};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

use super::Plugin;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ScriptConfig {
    pub script: String,
}

pub(crate) struct ScriptPlugin {
    program: rune::Unit,
}

impl ScriptPlugin {
    pub fn new(cfg: ScriptConfig) -> Result<Self, ConfigError> {
        let context = rune::Context::with_default_modules()
            .map_err(|e| ConfigError::Message(format!("{:?}", e)))?;

        let mut sources = rune::Sources::new();
        sources.insert(rune::Source::new("ScriptPlugin", &cfg.script));

        let mut diagnostics = rune::Diagnostics::new();

        let program = rune::prepare(&mut sources)
            .with_context(&context)
            .with_diagnostics(&mut diagnostics)
            .build()
            .map_err(|e| ConfigError::Message(format!("script compile err: {:?}", e)))?;

        Ok(ScriptPlugin { program })
    }
}
