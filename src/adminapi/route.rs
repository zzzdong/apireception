use lieweb::Json;

use super::{status::Status, ApiCtx, ApiParam, ApiResult};
use crate::config::RouteConfig;

type RouteCfg = Json<RouteConfig>;

pub struct RouteApi;

impl RouteApi {
    pub async fn get_detail(app_ctx: ApiCtx, param: ApiParam) -> ApiResult<RouteConfig> {
        let route_id = &param.value().id;

        let config = app_ctx.registry_reader.get().config;

        let route = config
            .routes
            .iter()
            .find(|r| &r.id == route_id)
            .cloned()
            .ok_or_else(|| Status::not_found("Route not exist"))?;

        Ok(route.into())
    }

    pub async fn get_list(app_ctx: ApiCtx) -> ApiResult<Vec<RouteConfig>> {
        let config = app_ctx.registry_reader.get().config;

        Ok(config.routes.clone().into())
    }

    pub async fn add(app_ctx: ApiCtx, route: RouteCfg) -> ApiResult<RouteConfig> {
        let route: RouteConfig = route.take();

        let mut config = app_ctx.registry.config.write().unwrap();

        if config.routes.iter().any(|r| r.id == route.id) {
            return Err(Status::bad_request("Route Id exist"));
        }

        config.routes.push(route.clone());

        app_ctx.registry_notify.notify_one();

        Ok(route.into())
    }

    pub async fn update(
        app_ctx: ApiCtx,
        param: ApiParam,
        route: RouteCfg,
    ) -> ApiResult<RouteConfig> {
        let mut route = route.take();
        let route_id = param.take().id;

        route.id = route_id;

        let mut config = app_ctx.registry.config.write().unwrap();

        match config.routes.iter_mut().find(|r| r.id == route.id) {
            Some(r) => {
                let _ = std::mem::replace(r, route.clone());
            }
            None => {
                return Err(Status::not_found("Route not exist"));
            }
        }

        app_ctx.registry_notify.notify_one();

        Ok(route.into())
    }
}
