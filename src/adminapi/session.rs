use std::{
    collections::HashMap,
    convert::TryInto,
    sync::{Arc, RwLock},
    time::Duration,
};

use hyper::StatusCode;
use lieweb::{middleware::Middleware, Cookie, Request, Response};
use lieweb::{Json, LieRequest, LieResponse};
use rand::Rng;
use serde::{Deserialize, Serialize};

use super::status::Status;

const ALLOWED_ADMIN: (&str, &str) = ("admin", "admin");
const SESSION_COOKIE_NAME: &str = "sid";

lazy_static::lazy_static! {
    static ref G_SESSION_STORE: Arc<RwLock<SessionStore<String>>> = Arc::new(RwLock::new(SessionStore::new()));
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

pub struct AuthMiddleware {
    login_path: String,
}

impl AuthMiddleware {
    pub fn new(login_path: impl ToString) -> Self {
        AuthMiddleware {
            login_path: login_path.to_string(),
        }
    }
}

#[lieweb::async_trait]
impl Middleware for AuthMiddleware {
    async fn handle<'a>(&'a self, req: Request, next: lieweb::middleware::Next<'a>) -> Response {
        if req.path() != self.login_path {
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

        return LieResponse::with_status(StatusCode::UNAUTHORIZED).into();
    }
}

pub struct SessionApi;

impl SessionApi {
    pub async fn login(req: Json<LoginReq>) -> Result<LieResponse, Status> {
        let login_req: LoginReq = req.take();

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

            let mut cookie = Cookie::new(SESSION_COOKIE_NAME, sid);
            cookie.set_path("/");

            let data = LoginResp { login_name };

            return Ok(LieResponse::with_json(data).append_cookie(cookie));
        }

        Err(Status::unauthorized("invalid user or password"))
    }

    pub async fn logout(req: Request) -> Result<LieResponse, Status> {
        if let Ok(ref cookie) = req.get_cookie(SESSION_COOKIE_NAME) {
            G_SESSION_STORE.clone().write().unwrap().delete(cookie);
        }

        let max_age = Duration::from_secs(0).try_into().unwrap();
        let mut cookie = Cookie::new(SESSION_COOKIE_NAME, "");
        cookie.set_max_age(Some(max_age));

        let resp = LieResponse::with_status(StatusCode::OK).append_cookie(cookie);

        Ok(resp)
    }
}

#[derive(Debug, Deserialize)]
pub struct LoginReq {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResp {
    pub login_name: String,
}
