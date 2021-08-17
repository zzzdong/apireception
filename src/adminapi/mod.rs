mod route;
mod session;
mod upstream;

use std::sync::{Arc, RwLock};

use arc_swap::ArcSwap;
use hyper::StatusCode;
use lieweb::{Error, Request, Response};
use serde::Serialize;
use tokio::sync::Notify;

use crate::config::{Config, RuntimeConfig, SharedData};

use self::{
    route::RouteApi,
    session::{AuthMiddleware, SessionApi},
    upstream::UpstreamApi,
};

#[derive(Debug, Serialize)]
struct IResponse<T: Serialize> {
    pub err_code: i32,
    pub err_msg: String,
    pub data: T,
}

impl<T> IResponse<T>
where
    T: Serialize,
{
    pub fn new(data: T) -> IResponse<T> {
        IResponse {
            err_code: 0,
            err_msg: String::from("ok"),
            data,
        }
    }
}

impl IResponse<Option<()>> {
    pub fn with_error(err_code: i32, err_msg: impl ToString) -> IResponse<Option<()>> {
        IResponse::<Option<()>> {
            err_code,
            err_msg: err_msg.to_string(),
            data: None::<()>,
        }
    }
}

impl<T: Serialize> From<IResponse<T>> for Response {
    fn from(r: IResponse<T>) -> Self {
        Response::with_json(&r)
    }
}

fn wrap_response<Resp>(resp: Result<Resp, Error>) -> Response
where
    Resp: Serialize,
{
    match resp {
        Ok(data) => {
            let resp = IResponse::new(data);
            Response::with_json(&resp)
        }
        Err(err) => {
            tracing::error!(%err, "handle request failed");
            let resp = IResponse::with_error(-1, err);
            Response::with_json(&resp).set_status(StatusCode::BAD_REQUEST)
        }
    }
}

#[derive(Clone)]
pub struct AppContext {
    config: Arc<RwLock<Config>>,
    config_notify: Arc<Notify>,
    shared_data: Arc<ArcSwap<SharedData>>,
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
            ..
        } = self.rtcfg;

        let app_ctx = AppContext {
            config,
            config_notify,
            shared_data,
        };

        let mut app = lieweb::App::with_state(app_ctx);

        app.middleware(AuthMiddleware);

        app.post("/api/session/login", |req: Request| async move {
            SessionApi::login(req).await
        });

        app.post("/api/session/logout", |req: Request| async move {
            SessionApi::logout(req).await
        });

        app.get("/api/routes", |req: Request| async move {
            wrap_response(RouteApi::get_list(req))
        });

        app.post("/api/routes", |req| async move {
            wrap_response(RouteApi::add(req).await)
        });

        app.get("/api/routes/:id", |req: Request| async move {
            wrap_response(RouteApi::get_detail(req))
        });

        app.put("/api/routes/:id", |req| async move {
            wrap_response(RouteApi::update(req).await)
        });

        app.get("/api/upstreams", |req: Request| async move {
            wrap_response(UpstreamApi::get_list(req))
        });

        app.post("/api/upstreams", |req| async move {
            wrap_response(UpstreamApi::add(req).await)
        });

        app.get("/api/upstreams/:id", |req: Request| async move {
            wrap_response(UpstreamApi::get_detail(req))
        });

        app.put("/api/upstreams/:id", |req| async move {
            wrap_response(UpstreamApi::update(req).await)
        });

        app.run("0.0.0.0:8000").await
    }
}
