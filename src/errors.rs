use std::fmt;

use actix_web::{error::ResponseError, HttpResponse};

use serde::Serialize;

#[derive(Serialize)]
struct JsonError {
    // causes a lot of code duplication (logical)
    status: u16,
    message: String,
}

#[derive(Debug)]
pub enum ServiceError {
    NotFound,
    InternalServerError,
    BadRequest(String),
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InternalServerError => {
                write!(f, "InternalServerError")
            }
            Self::BadRequest(_) => write!(f, "BadRequest"),
            Self::NotFound => write!(f, "NotFound"),
        }
    }
}

impl ResponseError for ServiceError {
    fn error_response(&self) -> HttpResponse {
        match self {
            Self::InternalServerError => HttpResponse::InternalServerError().json(JsonError {
                status: 500,
                message: "Internal Server Error".into(),
            }),
            Self::BadRequest(message) => HttpResponse::BadRequest().json(JsonError {
                status: 400,
                message: message.into(),
            }),
            Self::NotFound => HttpResponse::NotFound().json(JsonError {
                status: 404,
                message: "Resource not found".into(),
            }),
        }
    }
}
