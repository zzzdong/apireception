mod route;
mod session;
mod status;
mod upstream;

use std::sync::{Arc, RwLock};

use hyper::StatusCode;
use lieweb::{response::IntoResponse, Error, LieResponse, Request, Response};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::config::{Config, RuntimeConfig, SharedData};

use self::{
    route::RouteApi,
    session::{AuthMiddleware, SessionApi},
    status::Status,
    upstream::UpstreamApi,
};

#[derive(Debug, Deserialize)]
pub struct IdParam {
    pub id: String,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub err_code: i32,
    pub err_msg: String,
    pub data: T,
    #[serde(skip)]
    pub status: Option<StatusCode>,
}

impl<T> ApiResponse<T>
where
    T: Serialize,
{
    pub fn new(data: T) -> ApiResponse<T> {
        ApiResponse {
            err_code: 0,
            err_msg: String::from("ok"),
            data,
            status: None,
        }
    }
}

impl<T: Serialize + Default> ApiResponse<T> {
    pub fn with_error(err_code: i32, err_msg: impl ToString) -> Self {
        ApiResponse {
            err_code,
            err_msg: err_msg.to_string(),
            data: T::default(),
            status: None,
        }
    }

    fn with_status(mut self, status: StatusCode) -> Self {
        self.status = Some(status);
        self
    }
}

impl<T: Serialize> From<ApiResponse<T>> for Response {
    fn from(r: ApiResponse<T>) -> Self {
        LieResponse::with_json(&r).into()
    }
}

impl From<lieweb::Error> for ApiResponse<Option<()>> {
    fn from(err: Error) -> Self {
        match err {
            Error::MissingParam { .. }
            | Error::InvalidParam { .. }
            | Error::MissingHeader { .. }
            | Error::InvalidHeader { .. }
            | Error::MissingCookie { .. } => {
                ApiResponse::with_error(400, err).with_status(StatusCode::BAD_REQUEST)
            }
            Error::JsonError(_) | Error::QueryError(_) => {
                ApiResponse::with_error(400, err).with_status(StatusCode::BAD_REQUEST)
            }
            _ => ApiResponse::with_error(500, err).with_status(StatusCode::INTERNAL_SERVER_ERROR),
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

type ApiResult<T> = Result<ApiResponse<T>, Status>;

#[derive(Clone)]
pub struct AppContext {
    config: Arc<RwLock<Config>>,
    config_notify: Arc<Notify>,
    shared_data: SharedData,
}

pub struct AdminApi {
    rtcfg: RuntimeConfig,
}

impl AdminApi {
    pub fn new(rtcfg: RuntimeConfig) -> Self {
        AdminApi { rtcfg }
    }

    pub async fn run(self) -> Result<(), Error> {
        let RuntimeConfig {
            config,
            shared_data,
            config_notify,
            adminapi_addr,
            ..
        } = self.rtcfg;

        let app_ctx = AppContext {
            config,
            config_notify,
            shared_data,
        };

        let mut app = lieweb::App::with_state(app_ctx);

        app.middleware(AuthMiddleware::new("/login"));

        app.post("/api/session/login", |req: Request| async move {
            SessionApi::login(req).await
        });

        app.post("/api/session/logout", |req: Request| async move {
            SessionApi::logout(req).await
        });

        app.get("/api/routes", RouteApi::get_list);

        app.post("/api/routes", RouteApi::add);

        app.get("/api/routes/:id", RouteApi::get_detail);

        app.put("/api/routes/:id", RouteApi::update);

        app.get("/api/upstreams", UpstreamApi::get_list);

        app.post("/api/upstreams", UpstreamApi::add);

        app.get("/api/upstreams/:id", UpstreamApi::get_detail);

        app.put("/api/upstreams/:id", UpstreamApi::update);

        app.run(adminapi_addr.unwrap()).await
    }
}
