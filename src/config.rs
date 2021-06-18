use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
struct Config {
    server: Server,
    routes: Vec<Route>,
}

#[derive(Debug, Clone, Deserialize)]
struct Server {
    addr: String,
    log_level: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Route {
    name: String,
    uris: Vec<String>,
    upstream: Upstream,

    modify_request: Option<String>,
    modify_response: Option<String>,
    forward_request: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Upstream {
    name: String,
    addrs: Vec<(String,u32)>,
    strategy: String,
}


struct Context {
    
}