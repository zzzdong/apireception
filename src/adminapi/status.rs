use lieweb::{response::IntoResponse, Error, LieResponse};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Status {
    pub code: i32,
    pub message: String,
}

impl Status {
    pub fn new(code: i32, message: impl ToString) -> Self {
        Status {
            code,
            message: message.to_string(),
        }
    }
}

impl Default for Status {
    fn default() -> Self {
        Status::new(0, "ok")
    }
}

impl From<lieweb::Error> for Status {
    fn from(err: Error) -> Self {
        match err {
            Error::MissingParam { .. }
            | Error::InvalidParam { .. }
            | Error::MissingHeader { .. }
            | Error::InvalidHeader { .. }
            | Error::MissingCookie { .. } => Status::new(400, err),
            Error::JsonError(_) | Error::QueryError(_) => Status::new(400, err),
            _ => Status::new(500, err),
        }
    }
}

impl IntoResponse for Status {
    fn into_response(self) -> lieweb::Response {
        LieResponse::with_json(&self).into_response()
    }
}
