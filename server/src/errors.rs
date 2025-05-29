use vectorize_core::errors;

use actix_web::{HttpResponse, ResponseError, http::StatusCode, web::JsonConfig};
use serde::Serialize;
use serde::ser::SerializeMap;
use serde_json;
use sqlx;
use thiserror::Error;
use utoipa::ToSchema;

#[derive(Error, Debug)]
pub enum ServerError {
    #[error("VectorizeError: {0}")]
    VectorizeError(#[from] errors::VectorizeError),
    #[error("DatabaseError: {0}")]
    NotFoundError(String),
    #[error("DatabaseError: {0}")]
    DatabaseError(#[from] sqlx::Error),
    #[error("InvalidRequest: {0}")]
    InvalidRequest(String),
    #[error("HTTP error: {0}")]
    Reqwest(#[from] reqwest::Error),
    // serde error
    #[error("Serialization error: {0}")]
    SerdeError(#[from] serde_json::Error),
}

// public facing http errors
#[derive(Error, Debug)]
pub enum ErrorResponse {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    NotAuthorized(String),
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    InternalServerError(String),
}

// FOR DOC ONLY
#[allow(dead_code)]
#[derive(ToSchema)]
pub struct ErrorResponseSchema {
    pub error: String,
}

impl Serialize for ErrorResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant_str = format!("{}", self);
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("error", &variant_str)?;
        map.end()
    }
}

impl ResponseError for ServerError {
    fn error_response(&self) -> HttpResponse {
        let resp = match self {
            ServerError::InvalidRequest(_) => ErrorResponse::BadRequest(self.to_string()),
            ServerError::NotFoundError(_) => ErrorResponse::NotFound(self.to_string()),
            _ => ErrorResponse::InternalServerError(
                "Internal Server Error. Check server logs".to_string(),
            ),
        };
        HttpResponse::build(self.status_code()).json(resp)
    }
    fn status_code(&self) -> StatusCode {
        match *self {
            ServerError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            ServerError::NotFoundError(_) => StatusCode::NOT_FOUND,
            _ => {
                log::error!("Internal Server Error: {:?}", self);
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }
}

pub fn make_json_config() -> JsonConfig {
    use actix_web::error::InternalError;

    JsonConfig::default().error_handler(|error, _request| {
        #[derive(Serialize)]
        struct ErrorBody<T: Serialize> {
            error: T,
        }

        let error_msg = error.to_string();
        let error_body = ErrorBody { error: error_msg };
        let response = HttpResponse::BadRequest().json(error_body);

        InternalError::from_response(error, response).into()
    })
}
