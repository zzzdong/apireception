use std::sync::Arc;

use headers::{HeaderName, HeaderValue};
use hyper::Body;
use rune::{runtime::RuntimeContext, ContextError, FromValue, Module, Unit, Vm};
use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

use super::Plugin;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ScriptConfig {
    pub script: String,
}

pub(crate) struct ScriptPlugin {
    unit: Arc<Unit>,
    runtime: Arc<RuntimeContext>,
}

impl ScriptPlugin {
    pub fn new(cfg: ScriptConfig) -> Result<Self, ConfigError> {
        let mut context = rune::Context::with_default_modules()
            .map_err(|e| ConfigError::Message(format!("{:?}", e)))?;

        let m = build_module().unwrap();
        context.install(&m).unwrap();

        let runtime = Arc::new(context.runtime());

        let mut sources = rune::Sources::new();
        sources.insert(rune::Source::new("entry", &cfg.script));

        let mut diagnostics = rune::Diagnostics::new();

        let unit = rune::prepare(&mut sources)
            .with_context(&context)
            .with_diagnostics(&mut diagnostics)
            .build()
            .map_err(|e| {
                ConfigError::Message(format!(
                    "script compile err: {:?}",
                    diagnostics.diagnostics()
                ))
            })?;

        Ok(ScriptPlugin {
            unit: Arc::new(unit),
            runtime,
        })
    }
}

impl Plugin for ScriptPlugin {
    fn priority(&self) -> u32 {
        2000
    }

    fn on_access(
        &self,
        ctx: &mut crate::context::GatewayContext,
        req: crate::http::HyperRequest,
    ) -> Result<crate::http::HyperRequest, crate::http::HyperResponse> {
        let mut vm = Vm::new(self.runtime.clone(), self.unit.clone());

        let output = vm
            .call(&["on_access"], (MyRequest { inner: req },))
            .unwrap();

        type MyResult = Result<MyRequest, MyResponse>;

        let ret = MyResult::from_value(output).unwrap();

        ret.map(|r| r.inner).map_err(|r| r.inner)
    }

    fn after_forward(
        &self,
        ctx: &mut crate::context::GatewayContext,
        resp: crate::http::HyperResponse,
    ) -> crate::http::HyperResponse {
        resp
    }
}

fn build_module() -> Result<Module, ContextError> {
    let mut module = Module::new();

    module.ty::<MyRequest>()?;
    module.ty::<MyResponse>()?;

    module.function(&["MyResponse", "new"], MyResponse::new)?;

    Ok(module)
}

#[derive(Debug, rune::Any)]
struct MyRequest {
    inner: crate::http::HyperRequest,
}

impl MyRequest {
    fn get_header(&self, key: &str) -> Option<String> {
        self.inner
            .headers()
            .get(key)
            .and_then(|v| v.to_str().ok().map(|s| s.to_string()))
    }

    fn set_header(&mut self, key: &str, value: &str) {
        self.inner.headers_mut().insert(
            HeaderName::from_bytes(key.as_bytes()).unwrap(),
            HeaderValue::from_str(value).unwrap(),
        );
    }
}

#[derive(Debug, rune::Any)]
struct MyResponse {
    inner: crate::http::HyperResponse,
}

impl MyResponse {
    fn new() -> Self {
        MyResponse {
            inner: hyper::Response::builder()
                .body(Body::from("I am in scripting"))
                .unwrap(),
        }
    }
}
