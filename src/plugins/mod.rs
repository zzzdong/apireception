pub mod path_rewrite;
pub mod traffic_split;

use std::sync::Arc;
use std::collections::HashMap;

use crate::context::GatewayContext;
use crate::error::ConfigError;
use crate::http::{HyperRequest, HyperResponse};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use self::path_rewrite::PathRewriteConfig;
use self::path_rewrite::PathRewritePlugin;
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
        ctx: &mut GatewayContext,
        req: HyperRequest,
    ) -> Result<HyperRequest, HyperResponse> {
        let _ = ctx;
        Ok(req)
    }

    /// after forward request, check or rewrite response.
    fn after_forward(&self, ctx: &mut GatewayContext, resp: HyperResponse) -> HyperResponse {
        let _ = ctx;
        resp
    }
}

// pub fn init_plugin(plugin: &PluginItem) -> Result<Arc<Box<dyn Plugin + Send + Sync>>, ConfigError> {
//     let plugin: Box<dyn Plugin + Send + Sync> = match plugin {
//         PluginItem::PathRewrite(cfg) => Box::new(PathRewritePlugin::new(cfg)?),
//         PluginItem::TrafficSplit(cfg) => Box::new(TrafficSplitPlugin::new(cfg)?),
//     };

//     Ok(Arc::new(plugin))
// }

pub fn init_plugin(name: &str, cfg: Value) -> Result<Arc<Box<dyn Plugin + Send + Sync>>, ConfigError> {
    let plugin: Box<dyn Plugin + Send + Sync> = match name {
        "path_rewrite" => {
            Box::new(PathRewritePlugin::new(cfg)?)
        }
        "traffic_split" => {
            Box::new(TrafficSplitPlugin::new(cfg)?)
        }
        _ => {
            return Err(ConfigError::Message("Unkown plugin".to_string()));
        }
    };

    Ok(Arc::new(plugin))
}
