use lieweb::{Error, Request};

use crate::config::RouteConfig;

use super::{AppContext, status::Status};

pub struct RouteApi;

impl RouteApi {
    pub fn get_detail(req: Request) -> Result<Option<RouteConfig>, Status> {
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

    pub fn get_list(req: Request) -> Result<Vec<RouteConfig>, Status> {
        let config = req
            .get_state::<AppContext>()
            .expect("AppContext not found")
            .config
            .read()
            .unwrap();

        Ok(config.routes.clone())
    }

    pub async fn add(mut req: Request) -> Result<String, Status> {
        let route: RouteConfig = req.read_json().await?;

        let mut config = req
            .get_state::<AppContext>()
            .expect("AppContext not found")
            .config
            .write()
            .unwrap();

        if config.routes.iter().any(|r| r.id == route.id) {
            return Err(Status::new(400, "Route Id exist"));
        }

        let route_id = route.id.clone();

        config.routes.push(route);

        req.get_state::<AppContext>()
            .expect("AppContext not found")
            .config_notify
            .notify_one();

        Ok(route_id)
    }

    pub async fn update(mut req: Request) -> Result<String, Status> {
        let route_id: String = req.get_param("id")?;
        let mut route: RouteConfig = req.read_json().await?;

        route.id = route_id.clone();

        let mut config = req
            .get_state::<AppContext>()
            .expect("AppContext not found")
            .config
            .write()
            .unwrap();

        match config.routes.iter_mut().find(|r| r.id == route.id) {
            Some(r) => {
                let _ = std::mem::replace(r, route);
            }
            None => {
                return Err(Status::new(400, "Route Id exist"));
            }
        }

        req.get_state::<AppContext>()
            .expect("AppContext not found")
            .config_notify
            .notify_one();

        Ok(route_id)
    }
}
