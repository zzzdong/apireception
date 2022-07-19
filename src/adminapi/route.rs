use lieweb::{AppState, LieRequest, PathParam, Request};

use crate::config::RouteConfig;

use super::{status::Status, ApiResult, AppContext, IdParam};

pub struct RouteApi;

impl RouteApi {
    pub async fn get_detail(
        app_ctx: AppState<AppContext>,
        param: PathParam<IdParam>,
    ) -> ApiResult<Option<RouteConfig>> {
        let route_id = &param.value().id;

        let config = app_ctx.config.read().unwrap();

        let route = config.routes.iter().find(|r| &r.id == route_id).cloned();

        Ok(route.into())
    }

    pub async fn get_list(app_ctx: AppState<AppContext>) -> ApiResult<Vec<RouteConfig>> {
        let config = app_ctx.config.read().unwrap();

        Ok(config.routes.clone().into())
    }

    pub async fn add(app_ctx: AppState<AppContext>, mut req: Request) -> ApiResult<String> {
        let route: RouteConfig = req.read_json().await?;

        let mut config = app_ctx.config.write().unwrap();

        if config.routes.iter().any(|r| r.id == route.id) {
            return Err(Status::new(400, "Route Id exist"));
        }

        let route_id = route.id.clone();

        config.routes.push(route);

        app_ctx.config_notify.notify_one();

        Ok(route_id.into())
    }

    pub async fn update(
        app_ctx: AppState<AppContext>,
        param: PathParam<IdParam>,
        mut req: Request,
    ) -> ApiResult<String> {
        let route_id: String = req.get_param("id")?;
        let mut route: RouteConfig = req.read_json().await?;

        route.id = route_id.clone();

        let mut config = app_ctx.config.write().unwrap();

        match config.routes.iter_mut().find(|r| r.id == route.id) {
            Some(r) => {
                let _ = std::mem::replace(r, route);
            }
            None => {
                return Err(Status::new(400, "Route Id exist"));
            }
        }

        app_ctx.config_notify.notify_one();

        Ok(route_id.into())
    }
}
