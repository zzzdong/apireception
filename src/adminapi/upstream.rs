use lieweb::{Error, Request};

use crate::config::UpstreamConfig;

use super::AppContext;

pub struct UpstreamApi;

impl UpstreamApi {
    pub fn get_detail(req: Request) -> Result<Option<UpstreamConfig>, Error> {
        let upstream_id: String = req.get_param("id")?;

        let config = req
            .get_state::<AppContext>()
            .expect("AppContext not found")
            .config
            .read()
            .unwrap();

        let upstream = config
            .upstreams
            .iter()
            .find(|up| up.id == upstream_id)
            .cloned();

        Ok(upstream)
    }

    pub fn get_list(req: Request) -> Result<Vec<UpstreamConfig>, Error> {
        let config = req
            .get_state::<AppContext>()
            .expect("AppContext not found")
            .config
            .read()
            .unwrap();

        Ok(config.upstreams.clone())
    }

    pub async fn add(mut req: Request) -> Result<String, Error> {
        let upstream: UpstreamConfig = req.read_json().await?;

        let mut config = req
            .get_state::<AppContext>()
            .expect("AppContext not found")
            .config
            .write()
            .unwrap();

        if config.upstreams.iter().any(|up| up.id == upstream.id) {
            return Err(Error::Message("Route Id exist".to_string()));
        }

        let upstream_id = upstream.id.clone();

        config.upstreams.push(upstream);

        req.get_state::<AppContext>()
            .expect("AppContext not found")
            .config_notify
            .notify_one();

        Ok(upstream_id)
    }

    pub async fn update(mut req: Request) -> Result<String, Error> {
        let upstream_id: String = req.get_param("id")?;
        let mut upstream: UpstreamConfig = req.read_json().await?;
        upstream.id = upstream_id.clone();

        let mut config = req
            .get_state::<AppContext>()
            .expect("AppContext not found")
            .config
            .write()
            .unwrap();

        let upstream_id = upstream.id.clone();

        match config.upstreams.iter_mut().find(|up| up.id == upstream.id) {
            Some(up) => {
                let _ = std::mem::replace(up, upstream);
            }
            None => {
                return Err(Error::Message("Route Id not exist".to_string()));
            }
        }

        req.get_state::<AppContext>()
            .expect("AppContext not found")
            .config_notify
            .notify_one();

        Ok(upstream_id)
    }
}
