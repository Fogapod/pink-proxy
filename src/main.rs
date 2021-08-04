use actix_web::ResponseError;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tokio::time;

use actix_web::{get, middleware::Logger, post, web, App, HttpResponse, HttpServer, Responder};

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

#[derive(Debug, Clone)]
struct Proxy {
    url: String,
    valid_until: Instant,
}

#[derive(Debug)]
struct State {
    proxies: Mutex<HashMap<Uuid, Proxy>>,
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
        proxies.insert(
            id,
            Proxy {
                url,
                valid_until: Instant::now() + Duration::from_secs(ttl),
            },
        );
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

    let proxy = state
        .proxies
        .lock()
        .map_err(|_| errors::ServiceError::InternalServerError {})?
        .get(&id)
        .cloned()
        .ok_or_else(|| errors::ServiceError::BadRequest("bad id".into()))?;

    let request = client.get(proxy.url);

    let remote_response = request
        .send()
        .await
        .map_err(|e| errors::ServiceError::BadRequest(format!("request failed: {}", e)))?;

    let mut response = HttpResponse::build(remote_response.status());

    remote_response.headers().iter().for_each(|(k, v)| {
        // O(nm), though there are only 2 ignored headers now, so overhead is not big
        if !IGNORED_HEADERS.contains(k) {
            response.insert_header((k, v.clone()));
        }
    });

    // TODO: stop if body too large?
    Ok(response.streaming(remote_response))
}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    std::env::set_var("RUST_LOG", "info");
    std::env::set_var("RUST_BACKTRACE", "1");

    dotenv::dotenv().ok();

    env_logger::init();

    let _guard = sentry::init((
        std::env::var("SENTRY_DSN").expect("SENTRY_DSN not set"),
        sentry::ClientOptions {
            release: sentry::release_name!(),
            ..Default::default()
        },
    ));

    let state = web::Data::new(State {
        proxies: Mutex::new(HashMap::new()),
    });

    // avoid move to server
    let cloned_state = state.clone();

    let server = HttpServer::new(move || {
        let logger = Logger::default();

        let json_config = web::JsonConfig::default()
            .limit(4096)
            .error_handler(|err, _req| {
                dbg!(&err);
                errors::ServiceError::BadRequest(err.to_string()).into()
            });

        let path_config = web::PathConfig::default().error_handler(|err, _req| {
            dbg!(&err);
            errors::ServiceError::BadRequest(err.to_string()).into()
        });

        App::new()
            .wrap(sentry_actix::Sentry::new())
            .wrap(logger)
            .app_data(json_config)
            .app_data(path_config)
            .app_data(web::Data::new(
                awc::ClientBuilder::new()
                    .disable_redirects()
                    .wrap(awc::middleware::Redirect::new().max_redirect_times(10))
                    .finish(),
            ))
            .app_data(cloned_state.clone())
            .service(web::scope("/proxy").service(post_proxy).service(get_proxy))
            .default_service(web::route().to(|| errors::ServiceError::NotFound {}.error_response()))
    })
    .bind("0.0.0.0:8000")?
    .run();

    // proxy cleanup task
    actix_rt::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(60));

        loop {
            interval.tick().await;

            let now = Instant::now();

            // if this panics, task dies, but app continues to process requests which
            // might not be desired. sentry does not capture this as well
            let mut proxies = state.proxies.lock().expect("task failed to unlock proxies");

            // this is O(n) which is very very very bad, need to maintain a separate
            // sorted set of valid instants probably
            proxies.retain(|_, v| v.valid_until > now);
        }
    });

    server.await
}
