use std::collections::HashMap;
use std::sync::Mutex;

use actix_web::{
    error, get, middleware::Logger, post, web, App, HttpResponse, HttpServer, Responder,
};

use serde::{Deserialize, Serialize};

use uuid::Uuid;

mod errors;

const IGNORED_HEADERS: &[awc::http::HeaderName] = &[
    awc::http::header::CONTENT_LENGTH,
    awc::http::header::CONTENT_ENCODING,
];

#[derive(Deserialize)]
struct ProxyURL {
    url: String,
    ttl: u64,
}

#[derive(Serialize)]
struct ProxyID {
    id: Uuid,
    ttl: u64,
}

#[derive(Debug)]
struct State {
    proxies: Mutex<HashMap<Uuid, String>>,
}

#[post("")]
async fn post_proxy(
    data: web::Json<ProxyURL>,
    state: web::Data<State>,
) -> Result<impl Responder, errors::ServiceError> {
    let ProxyURL { url, ttl } = data.into_inner();

    let id = Uuid::new_v4();

    {
        let mut proxies = state
            .proxies
            .lock()
            .map_err(|_| errors::ServiceError::InternalServerError {})?;
        proxies.insert(id, url);
    }

    Ok(HttpResponse::Ok().json(ProxyID { id, ttl }))
}

#[get("{id}")]
async fn get_proxy(
    path: web::Path<Uuid>,
    state: web::Data<State>,
    client: web::Data<awc::Client>,
) -> Result<impl Responder, errors::ServiceError> {
    let id = path.into_inner();

    let url = state
        .proxies
        .lock()
        .map_err(|_| errors::ServiceError::InternalServerError {})?
        .get(&id)
        .cloned()
        .ok_or_else(|| errors::ServiceError::BadRequest("bad id".into()))?;

    let request = client.get(url);

    let remote_response = request
        .send()
        .await
        .map_err(|e| errors::ServiceError::BadRequest(format!("request failed: {}", e)))?;

    let mut response = HttpResponse::build(remote_response.status());

    remote_response.headers().iter().for_each(|(k, v)| {
        if !IGNORED_HEADERS.contains(k) {
            response.insert_header((k, v.clone()));
        }
    });

    // TODO: stop if body too large?
    Ok(response.streaming(remote_response))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // TODO: background worker, cleaning URL map

    let state = web::Data::new(State {
        proxies: Mutex::new(HashMap::new()),
    });

    std::env::set_var("RUST_LOG", "info");
    std::env::set_var("RUST_BACKTRACE", "1");
    env_logger::init();

    HttpServer::new(move || {
        let logger = Logger::default();

        let json_config = web::JsonConfig::default()
            .limit(4096)
            .error_handler(|err, _req| {
                dbg!(&err);
                errors::ServiceError::BadRequest(err.to_string()).into()
                // error::InternalError::from_response(err, HttpResponse::Conflict().finish()).into()
            });

        App::new()
            .wrap(logger)
            .app_data(json_config)
            .app_data(web::Data::new(
                awc::ClientBuilder::new()
                    .disable_redirects()
                    .wrap(awc::middleware::Redirect::new().max_redirect_times(10))
                    .finish(),
            ))
            .app_data(state.clone())
            .service(web::scope("/proxy").service(post_proxy).service(get_proxy))
    })
    .bind("0.0.0.0:8000")?
    .run()
    .await
}
