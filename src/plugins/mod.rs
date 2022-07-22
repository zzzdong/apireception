pub mod path_rewrite;
pub mod script;
pub mod traffic_split;

use std::sync::Arc;

use serde::de::DeserializeOwned;

use crate::context::GatewayInfo;
use crate::error::ConfigError;
use crate::http::{HyperRequest, HyperResponse};

pub use self::path_rewrite::PathRewriteConfig;
use self::path_rewrite::PathRewritePlugin;
pub use self::script::ScriptConfig;
use self::script::ScriptPlugin;
use self::traffic_split::TrafficSplitPlugin;
pub use self::traffic_split::{TrafficSplitConfig, TrafficSplitRule};

pub trait Plugin {
    /// Get plugin name.
    fn name(&self) -> &str {
        std::any::type_name::<Self>()
    }

    /// Get pluign priority.
    fn priority(&self) -> u32;

    /// when a request arrived, check or rewrite request.
    fn on_access(
        &self,
        ctx: &mut GatewayInfo,
        req: HyperRequest,
    ) -> Result<HyperRequest, HyperResponse> {
        let _ = ctx;
        Ok(req)
    }

    /// after forward request, check or rewrite response.
    fn after_forward(&self, ctx: &mut GatewayInfo, resp: HyperResponse) -> HyperResponse {
        let _ = ctx;
        resp
    }
}

fn parse_config<T: DeserializeOwned>(cfg: serde_json::Value) -> Result<T, ConfigError> {
    serde_json::from_value(cfg).map_err(Into::into)
}

pub fn init_plugin(
    name: &str,
    cfg: serde_json::Value,
) -> Result<Arc<Box<dyn Plugin + Send + Sync>>, ConfigError> {
    let plugin: Box<dyn Plugin + Send + Sync> = match name {
        "path_rewrite" => Box::new(PathRewritePlugin::new(parse_config(cfg)?)?),
        "traffic_split" => Box::new(TrafficSplitPlugin::new(parse_config(cfg)?)?),
        "script" => Box::new(ScriptPlugin::new(parse_config(cfg)?)?),
        _ => {
            return Err(ConfigError::Message("Unkown plugin".to_string()));
        }
    };

    Ok(Arc::new(plugin))
}
