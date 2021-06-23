use futures::future::join_all;
use tokio::net::TcpStream;

mod config;
mod matcher;
mod server;

const DEFAULT_ADDR: &str = "0.0.0.0:5000";

const WORKER_NUM: usize = 4;

fn main() {
    tracing_subscriber::fmt::init();

    // server::Server::run_good_server();

    let ret = run_server();

    println!("ret={:?}", ret);
}

fn run_server() -> anyhow::Result<()> {
    let (tx, rx) = crossbeam::channel::bounded(1);

    let listener = std::net::TcpListener::bind(DEFAULT_ADDR)?;

    for i in 0..WORKER_NUM {
        let rx_cloned = rx.clone();

        std::thread::spawn(move || {
            server::Server::run_in_thread(rx_cloned);

            // println!("thread {} started", i);
        });
    }

    while let Ok((socket, addr)) = listener.accept() {
        tx.send((socket, addr)).unwrap();
    }

    Ok(())
}
