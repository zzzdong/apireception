use lieweb::{extracts::JsonRejection, Json};

use super::{status::Status, ApiCtx, ApiParam, ApiResult};
use crate::config::UpstreamConfig;

type UpstreamCfg = Json<UpstreamConfig>;

pub struct UpstreamApi;

impl UpstreamApi {
    pub async fn get_detail(app_ctx: ApiCtx, param: ApiParam) -> ApiResult<UpstreamConfig> {
        let upstream_id = &param.value().id;

        let config = app_ctx.registry_cfg.read().unwrap();

        let upstream = config
            .upstreams
            .iter()
            .find(|up| &up.id == upstream_id)
            .cloned()
            .ok_or_else(|| Status::not_found("Upstream not exist"))?;

        Ok(upstream.into())
    }

    pub async fn get_list(app_ctx: ApiCtx) -> ApiResult<Vec<UpstreamConfig>> {
        let config = app_ctx.registry_cfg.read().unwrap();

        Ok(config.upstreams.clone().into())
    }

    pub async fn add(app_ctx: ApiCtx, upstream: UpstreamCfg) -> ApiResult<UpstreamConfig> {
        let upstream = upstream.take();

        let mut config = app_ctx.registry_cfg.write().unwrap();

        if config.upstreams.iter().any(|up| up.id == upstream.id) {
            return Err(Status::bad_request("Upstream Id exist"));
        }

        config.upstreams.push(upstream.clone());

        app_ctx.registry_notify.notify_one();

        Ok(upstream.into())
    }

    pub async fn update(
        app_ctx: ApiCtx,
        param: ApiParam,
        upstream: Result<Json<UpstreamConfig>, JsonRejection>,
    ) -> ApiResult<UpstreamConfig> {
        let mut upstream = upstream.map(|v| v.take()).map_err(Status::bad_request)?;
        let upstream_id = param.take().id;

        upstream.id = upstream_id;

        let mut config = app_ctx.registry_cfg.write().unwrap();

        match config.upstreams.iter_mut().find(|up| up.id == upstream.id) {
            Some(up) => {
                let _ = std::mem::replace(up, upstream.clone());
            }
            None => {
                return Err(Status::not_found("Upstream not exist"));
            }
        }

        app_ctx.registry_notify.notify_one();

        Ok(upstream.into())
    }
}
