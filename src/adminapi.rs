use std::{
    collections::HashMap,
    convert::TryInto,
    sync::{Arc, RwLock},
    time::Duration,
};

use arc_swap::ArcSwap;
use hyper::StatusCode;
use lieweb::{middleware::Middleware, Cookie, Error, Request, Response};
use rand::Rng;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::config::{Config, RouteConfig, SharedData};

const ALLOWED_ADMIN: (&str, &str) = ("admin", "admin");
const SESSION_COOKIE_NAME: &str = "sid";

lazy_static::lazy_static! {
    static ref G_SESSION_STORE: Arc<RwLock<SessionStore<String>>> = Arc::new(RwLock::new(SessionStore::new()));
}

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

#[derive(Debug, Deserialize)]
struct LoginReq {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
struct LoginResp {
    pub login_name: String,
}

async fn login(mut req: Request) -> Result<Response, Error> {
    let login_req: LoginReq = req.read_json().await?;

    if login_req.username == ALLOWED_ADMIN.0 && login_req.password == ALLOWED_ADMIN.1 {
        let login_name = login_req.username;

        let sid = rand::thread_rng().gen::<[u8; 8]>();
        let sid = sid
            .iter()
            .map(|b| format!("{:02x?}", b))
            .collect::<Vec<String>>()
            .join("");

        G_SESSION_STORE
            .clone()
            .write()
            .unwrap()
            .store(&sid, login_name.to_string());

        let cookie = Cookie::new(SESSION_COOKIE_NAME, sid);

        let data = LoginResp { login_name };
        let resp: Response = IResponse::new(data).into();

        return Ok(resp.append_cookie(cookie));
    }

    Ok(StatusCode::UNAUTHORIZED.into())
}

async fn logout(req: Request) -> Result<Response, Error> {
    if let Ok(ref cookie) = req.get_cookie(SESSION_COOKIE_NAME) {
        G_SESSION_STORE.clone().write().unwrap().delete(cookie);
    }

    let max_age = Duration::from_secs(0).try_into().unwrap();
    let mut cookie = Cookie::new(SESSION_COOKIE_NAME, "");
    cookie.set_max_age(Some(max_age));

    let resp = Response::with_status(StatusCode::OK);

    Ok(resp.append_cookie(cookie))
}

struct RouteApi;

impl RouteApi {
    pub fn get_detail(req: Request) -> Result<Option<RouteConfig>, Error> {
        let route_id: String = req.get_param("id")?;

        let config = req
            .get_state::<AppContext>()
            .expect("AppContext not found")
            .config
            .read()
            .unwrap();

        let route = config.routes.iter().find(|r| r.id == route_id).cloned();

        Ok(route)
    }

    pub fn get_list(req: Request) -> Result<Vec<RouteConfig>, Error> {
        let config = req
            .get_state::<AppContext>()
            .expect("AppContext not found")
            .config
            .read()
            .unwrap();

        Ok(config.routes.clone())
    }

    pub async fn add(mut req: Request) -> Result<String, Error> {
        let route: RouteConfig = req.read_json().await?;

        let mut config = req
            .get_state::<AppContext>()
            .expect("AppContext not found")
            .config
            .write()
            .unwrap();

        if config.routes.iter().any(|r| r.id == route.id) {
            return Err(Error::Message("Route Id exist".to_string()));
        }

        let route_id = route.id.clone();

        config.routes.push(route);

        req.get_state::<AppContext>()
            .expect("AppContext not found")
            .config_notify
            .notify_one();

        Ok(route_id)
    }

    pub async fn update(mut req: Request) -> Result<String, Error> {
        let route: RouteConfig = req.read_json().await?;

        let mut config = req
            .get_state::<AppContext>()
            .expect("AppContext not found")
            .config
            .write()
            .unwrap();

        let route_id = route.id.clone();

        match config.routes.iter_mut().find(|r| r.id == route.id) {
            Some(r) => {
                let _ = std::mem::replace(r, route);
            }
            None => {
                return Err(Error::Message("Route Id not exist".to_string()));
            }
        }

        req.get_state::<AppContext>()
            .expect("AppContext not found")
            .config_notify
            .notify_one();

        Ok(route_id)
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

struct AuthMiddleware;

#[lieweb::async_trait]
impl Middleware for AuthMiddleware {
    async fn handle<'a>(&'a self, req: Request, next: lieweb::middleware::Next<'a>) -> Response {
        if req.path().split('/').last().unwrap_or_default() != "login" {
            if let Ok(ref cookie) = req.get_cookie(SESSION_COOKIE_NAME) {
                let session = {
                    let session_store = G_SESSION_STORE.clone();
                    let session = session_store.read().unwrap();
                    session.load(cookie).cloned()
                };

                if let Some(_session) = session {
                    let resp = next.run(req).await;
                    return resp;
                }
            }
        } else {
            return next.run(req).await;
        }

        return StatusCode::UNAUTHORIZED.into();
    }
}

struct SessionStore<T> {
    map: HashMap<String, T>,
}

impl<T> SessionStore<T> {
    fn new() -> Self {
        SessionStore {
            map: HashMap::new(),
        }
    }

    fn load(&self, key: &str) -> Option<&T> {
        self.map.get(key)
    }

    fn store(&mut self, key: &str, value: T) {
        self.map.insert(key.to_string(), value);
    }

    fn delete(&mut self, key: &str) -> Option<T> {
        self.map.remove(key)
    }
}

#[derive(Clone)]
struct AppContext {
    config: Arc<RwLock<Config>>,
    config_notify: Arc<Notify>,
    shared_data: Arc<ArcSwap<SharedData>>,
}

pub async fn run(
    config: Arc<RwLock<Config>>,
    shared_data: Arc<ArcSwap<SharedData>>,
    config_notify: Arc<Notify>,
) {
    let app_ctx = AppContext {
        config,
        config_notify,
        shared_data,
    };

    let mut app = lieweb::App::with_state(app_ctx);

    app.middleware(AuthMiddleware);

    app.post("/api/session/login", |req: Request| async move {
        login(req).await
    });

    app.post("/api/session/logout", |req: Request| async move {
        logout(req).await
    });

    app.get("/api/routes", |req: Request| async move {
        wrap_response(RouteApi::get_list(req))
    });

    app.get("/api/routes/:id", |req: Request| async move {
        wrap_response(RouteApi::get_detail(req))
    });

    app.put("/api/routes/:id", |req| async move {
        wrap_response(RouteApi::update(req).await)
    });

    app.post("/api/routes/:id", |req| async move {
        wrap_response(RouteApi::add(req).await)
    });

    app.run("0.0.0.0:8000").await.unwrap();
}