use lieweb::{http::StatusCode, response::IntoResponse, Error, LieResponse};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Status {
    pub code: i32,
    pub message: String,
    #[serde(skip)]
    pub status: StatusCode,
}

impl Status {
    pub fn new(code: i32, message: impl ToString, status: StatusCode) -> Self {
        Status {
            code,
            message: message.to_string(),
            status,
        }
    }

    pub fn bad_request(message: impl ToString) -> Self {
        Status {
            code: 10400,
            message: message.to_string(),
            status: StatusCode::BAD_REQUEST,
        }
    }

    pub fn unauthorized(message: impl ToString) -> Self {
        Status {
            code: 10401,
            message: message.to_string(),
            status: StatusCode::UNAUTHORIZED,
        }
    }

    pub fn not_found(message: impl ToString) -> Self {
        Status {
            code: 10404,
            message: message.to_string(),
            status: StatusCode::NOT_FOUND,
        }
    }

    pub fn internal_error(message: impl ToString) -> Self {
        Status {
            code: 10500,
            message: message.to_string(),
            status: StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<lieweb::Error> for Status {
    fn from(err: Error) -> Self {
        match err {
            Error::MissingParam { .. }
            | Error::InvalidParam { .. }
            | Error::MissingHeader { .. }
            | Error::InvalidHeader { .. }
            | Error::MissingCookie { .. } => Status::bad_request(err),
            Error::JsonError(_) => Status::bad_request(err),
            _ => Status::internal_error(err),
        }
    }
}

impl IntoResponse for Status {
    fn into_response(self) -> lieweb::Response {
        let status = self.status;
        LieResponse::with_json(self)
            .set_status(status)
            .into_response()
    }
}
