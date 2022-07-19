use lieweb::{AppState, LieRequest, PathParam, Request};

use crate::config::UpstreamConfig;

use super::{status::Status, ApiResult, AppContext, IdParam};

pub struct UpstreamApi;

impl UpstreamApi {
    pub async fn get_detail(
        app_ctx: AppState<AppContext>,
        param: PathParam<IdParam>,
    ) -> ApiResult<Option<UpstreamConfig>> {
        let upstream_id = &param.value().id;

        let config = app_ctx.config.read().unwrap();

        let upstream = config
            .upstreams
            .iter()
            .find(|up| &up.id == upstream_id)
            .cloned();

        Ok(upstream.into())
    }

    pub async fn get_list(app_ctx: AppState<AppContext>) -> ApiResult<Vec<UpstreamConfig>> {
        let config = app_ctx.config.read().unwrap();

        Ok(config.upstreams.clone().into())
    }

    pub async fn add(app_ctx: AppState<AppContext>, mut req: Request) -> Result<String, Status> {
        let upstream: UpstreamConfig = req.read_json().await?;

        let mut config = app_ctx.config.write().unwrap();

        if config.upstreams.iter().any(|up| up.id == upstream.id) {
            return Err(Status::new(401, "Upstream Id exist"));
        }

        let upstream_id = upstream.id.clone();

        config.upstreams.push(upstream);

        app_ctx.config_notify.notify_one();

        Ok(upstream_id)
    }

    pub async fn update(
        app_ctx: AppState<AppContext>,
        param: PathParam<IdParam>,
        mut req: Request,
    ) -> Result<String, Status> {
        let upstream_id = &param.value().id;
        let mut upstream: UpstreamConfig = req.read_json().await?;
        upstream.id = upstream_id.clone();

        let mut config = app_ctx.config.write().unwrap();

        let upstream_id = upstream.id.clone();

        match config.upstreams.iter_mut().find(|up| up.id == upstream.id) {
            Some(up) => {
                let _ = std::mem::replace(up, upstream);
            }
            None => {
                return Err(Status::new(400, "Upstream Id exist"));
            }
        }

        app_ctx.config_notify.notify_one();

        Ok(upstream_id)
    }
}
