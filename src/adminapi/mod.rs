mod route;
mod session;
mod status;
mod upstream;

use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
};

use lieweb::{response::IntoResponse, AppState, Error, LieResponse, PathParam, Request, Response};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::config::{Config, RuntimeConfig, SharedData};

use self::{
    route::RouteApi,
    session::{AuthMiddleware, SessionApi},
    status::Status,
    upstream::UpstreamApi,
};

type ApiCtx = AppState<AppContext>;

type ApiParam = PathParam<Param>;

type ApiResult<T> = Result<ApiResponse<T>, Status>;

#[derive(Clone)]
pub struct AppContext {
    config: Arc<RwLock<Config>>,
    config_notify: Arc<Notify>,
    shared_data: SharedData,
}

#[derive(Debug, Deserialize)]
pub struct Param {
    pub id: String,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub err_code: i32,
    pub err_msg: String,
    pub data: Option<T>,
}

impl<T> ApiResponse<T>
where
    T: Serialize,
{
    pub fn new(data: T) -> ApiResponse<T> {
        ApiResponse {
            err_code: 0,
            err_msg: String::from("ok"),
            data: Some(data),
        }
    }
}

impl<T: Serialize + Default> ApiResponse<T> {
    pub fn with_error(err_code: i32, err_msg: impl ToString) -> Self {
        ApiResponse {
            err_code,
            err_msg: err_msg.to_string(),
            data: None,
        }
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        let resp = LieResponse::with_json(&self);
        resp.into_response()
    }
}

impl<T: Serialize> From<T> for ApiResponse<T> {
    fn from(t: T) -> Self {
        ApiResponse::new(t)
    }
}

pub struct AdminApi {
    rtcfg: RuntimeConfig,
}

impl AdminApi {
    pub fn new(rtcfg: RuntimeConfig) -> Self {
        AdminApi { rtcfg }
    }

    pub async fn run(self, addr: SocketAddr) -> Result<(), Error> {
        let RuntimeConfig {
            config,
            shared_data,
            config_notify,
            watch,
            ..
        } = self.rtcfg;

        let app_ctx = AppContext {
            config,
            config_notify,
            shared_data,
        };

        let mut app = lieweb::App::with_state(app_ctx);

        app.middleware(AuthMiddleware::new("/api/session/login"));

        app.post("/api/session/login", SessionApi::login);

        app.post("/api/session/logout", SessionApi::logout);

        app.get("/api/routes", RouteApi::get_list);

        app.post("/api/routes", RouteApi::add);

        app.get("/api/routes/:id", RouteApi::get_detail);

        app.put("/api/routes/:id", RouteApi::update);

        app.get("/api/upstreams", UpstreamApi::get_list);

        app.post("/api/upstreams", UpstreamApi::add);

        app.get("/api/upstreams/:id", UpstreamApi::get_detail);

        app.put("/api/upstreams/:id", UpstreamApi::update);

        tracing::info!("adminapi run on {:?}", addr);

        tokio::select! {
            _ = app.run(addr) => {

            }
            _shutdown = watch.signaled() => {

            }
        };

        Ok(())
    }
}
