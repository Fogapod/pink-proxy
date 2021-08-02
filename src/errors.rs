use std::fmt;

use actix_web::{error::ResponseError, HttpResponse};

use serde::Serialize;

#[derive(Serialize)]
struct JsonError {
    //status: u16,
    message: String,
}

#[derive(Debug)]
pub enum ServiceError {
    InternalServerError,
    BadRequest(String),
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ServiceError::InternalServerError => {
                write!(f, "InternalServerError")
            }
            ServiceError::BadRequest(_) => write!(f, "BadRequest"),
        }
    }
}

impl ResponseError for ServiceError {
    fn error_response(&self) -> HttpResponse {
        match self {
            ServiceError::InternalServerError => {
                HttpResponse::InternalServerError().json(JsonError {
                    message: "Internal Server Error".into(),
                })
            }
            ServiceError::BadRequest(message) => HttpResponse::BadRequest().json(JsonError {
                message: message.into(),
            }),
        }
    }
}
