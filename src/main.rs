use std::net::SocketAddr;

use futures::future::join_all;
use server::Server;
use tokio::net::TcpStream;

mod config;
mod matcher;
mod runtime_config;
mod server;

const DEFAULT_ADDR: &str = "0.0.0.0:5000";

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let srv = Server {};

    let addr = DEFAULT_ADDR.parse::<SocketAddr>().unwrap();

    let ret = srv.run(addr).await;

    println!("srv.run ret={:?}", ret);
}
