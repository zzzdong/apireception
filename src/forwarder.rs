use std::{fmt::Write, sync::Arc};

use headers::HeaderValue;
use hyper::{client::HttpConnector, header::HOST, http::uri::Scheme, Body, Client, Uri};
use hyper_rustls::HttpsConnector;
use tower::Service;

use crate::{
    context::GatewayContext,
    http::{HyperRequest, HyperResponse},
    load_balance::LoadBalanceStrategy,
};

#[derive(Clone)]
pub struct HttpClient {
    client: hyper::Client<HttpsConnector<HttpConnector>, Body>,
}

impl HttpClient {
    pub fn new() -> Self {
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();

        let inner: Client<_, hyper::Body> = Client::builder().build(https);

        HttpClient { client: inner }
    }

    pub async fn do_forward<'a>(
        &mut self,
        ctx: &'a GatewayContext,
        mut req: HyperRequest,
        endpoint: &Uri,
    ) -> Result<HyperResponse, hyper::Error> {
        let mut parts = endpoint.clone().into_parts();

        parts.scheme = Some(parts.scheme.unwrap_or(Scheme::HTTP));
        parts.path_and_query = req.uri().path_and_query().map(|p| p.clone());

        let uri = Uri::from_parts(parts).expect("build uri failed");

        *req.uri_mut() = uri;

        let resp = Service::call(&mut self.client, req).await;

        resp
    }
}

#[derive(Clone)]
pub struct Fowarder {
    client: HttpClient,
    pub(crate) strategy: Arc<Box<dyn LoadBalanceStrategy>>,
}

impl Fowarder {
    pub fn new(client: HttpClient, strategy: Arc<Box<dyn LoadBalanceStrategy>>) -> Self {
        Fowarder { client, strategy }
    }

    pub async fn forward(
        &mut self,
        ctx: &mut GatewayContext,
        mut req: HyperRequest,
    ) -> Result<HyperResponse, crate::Error> {
        // add forward info
        Self::append_proxy_headers(ctx, &mut req);

        if ctx.overwrite_host {
            let host = req.uri().host().expect("get host failed");
            let host = HeaderValue::from_str(host).expect("HeaderValue failed");
            req.headers_mut().insert(HOST, host);
        }

        let endpoint = self.strategy.select_endpoint(ctx, &req).to_owned();

        self.strategy.on_send_request(&ctx, &endpoint);

        let resp = self.client.do_forward(ctx, req, &endpoint).await;

        self.strategy.on_request_done(&ctx, &endpoint);

        resp.map_err(Into::into)
    }

    fn append_proxy_headers(ctx: &GatewayContext, req: &mut HyperRequest) {
        let x_forwarded_for = req.headers().get(crate::http::X_FORWARDED_FOR);

        if let Some(remote_addr) = ctx.remote_addr {
            let x_forwarded_for = match x_forwarded_for {
                Some(exist_forwarded_for) => {
                    let mut forwarded_for = exist_forwarded_for.to_str().unwrap_or("").to_string();
                    write!(forwarded_for, ", {}", remote_addr).unwrap();
                    forwarded_for
                }
                None => remote_addr.to_string(),
            };

            req.headers_mut().insert(
                crate::http::X_FORWARDED_FOR,
                HeaderValue::from_str(&x_forwarded_for).expect("HeaderValue failed"),
            );

            req.headers_mut().insert(
                crate::http::X_REAL_IP,
                HeaderValue::from_str(&remote_addr.ip().to_string()).expect("HeaderValue failed"),
            );
        }

        req.headers_mut().insert(
            crate::http::X_FORWARDED_PROTO,
            HeaderValue::from_str(ctx.orig_scheme.as_str()).expect("HeaderValue failed"),
        );

        if let Some(ref host) = ctx.orig_host {
            req.headers_mut().insert(
                crate::http::X_FORWARDED_HOST,
                HeaderValue::from_str(host).expect("HeaderValue failed"),
            );
        }
    }
}
