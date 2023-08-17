// mod adminapi;
mod config;
mod context;
mod error;
mod forwarder;
mod health;
mod http;
mod load_balance;
mod matcher;
mod peer_addr;
mod plugins;
mod registry;
mod router;
mod server;
mod services;
mod trace;
mod upstream;

use std::process::exit;

pub use error::{Error, Result};

use hyper::http::uri::Scheme;
use server::Server;

use crate::server::ServerContext;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    match run().await {
        Ok(_) => {
            println!("server run done, exit...");
        }
        Err(e) => {
            println!("server run error: {:?}", e);
        }
    }
}

async fn run() -> Result<()> {
    let cfg = config::Config::load_file("config/config.yaml")?;

    tracing::debug!(?cfg, "load config done");

    let (drain_tx, drain_rx) = drain::channel();
    let srv_ctx = ServerContext::new(cfg, drain_rx).await?;

    // srv_ctx.start_watch_registry();

    let srv_ctx_cloned = srv_ctx.clone();

    // Serve HTTP
    tokio::spawn(async move {
        let srv = Server::new(Scheme::HTTP, srv_ctx_cloned.registry_reader);
        let ret = srv
            .run(srv_ctx_cloned.http_addr, srv_ctx_cloned.watch)
            .await;

        match ret {
            Ok(_) => {
                tracing::info!("http server done");
            }
            Err(err) => {
                tracing::error!(?err, "http server error");
                exit(1);
            }
        }
    });

    // TODO: add serve https
    // let srv_ctx_cloned = srv_ctx.clone();

    // if srv_ctx_cloned.config.admin.enable {
    //     let adminapi_addr = srv_ctx.adminapi_addr.unwrap();
    //     tokio::spawn(async move {
    //         let adminapi = AdminApi::new(srv_ctx_cloned);
    //         match adminapi.run(adminapi_addr).await {
    //             Ok(_) => {
    //                 tracing::info!("adminapi server done");
    //             }
    //             Err(err) => {
    //                 tracing::error!(?err, "adminapi server error");
    //             }
    //         }
    //     });
    // }

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("got ctrl_c, shutting down...");
            let _shutdown = srv_ctx.watch.ignore_signaled();
        }
    }

    drain_tx.drain().await;

    Ok(())
}
