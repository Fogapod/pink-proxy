mod errors;
//mod middlewares;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::{sync::Mutex, time};

use actix_web::{get, post};
use actix_web::{
    http::header, middleware, web, App, Error, HttpRequest, HttpResponse, HttpServer, Responder,
    ResponseError,
};

use serde::{Deserialize, Serialize};

use uuid::Uuid;

use constant_time_eq::constant_time_eq;

use errors::ServiceError;

const IGNORED_HEADERS: &[awc::http::HeaderName] = &[awc::http::header::CONTENT_LENGTH];

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
struct Proxies(Mutex<HashMap<Uuid, Proxy>>);

fn authorize(rq: &HttpRequest) -> Result<(), ServiceError> {
    let auth_header =
        rq.headers()
            .get(header::AUTHORIZATION)
            .ok_or_else(|| ServiceError::Unauthorized {
                message: "missing Authorization header".into(),
            })?;

    let token = auth_header
        .to_str()
        .map_err(|_| ServiceError::Unauthorized {
            message: "bad Authorization header".into(),
        })?
        .strip_prefix("Bearer ")
        .ok_or_else(|| ServiceError::Unauthorized {
            message: "bad Bearer token format".into(),
        })?;

    // TODO: is it expensive? move to state?
    let master_token = std::env::var("ACCESS_TOKEN").expect("ACCESS_TOKEN not set");

    if !constant_time_eq(token.as_bytes(), master_token.as_bytes()) {
        return Err(ServiceError::Unauthorized {
            message: "bad token".into(),
        });
    }

    Ok(())
}

#[post("")]
async fn post_proxy(
    rq: HttpRequest,
    data: web::Json<ProxyURL>,
    proxies: web::Data<Proxies>,
) -> Result<impl Responder, Error> {
    authorize(&rq)?;

    let ProxyURL { url, ttl } = data.into_inner();

    let id = Uuid::new_v4();

    {
        let mut proxies = proxies.0.lock().await;
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
    proxies: web::Data<Proxies>,
    client: web::Data<awc::Client>,
) -> Result<impl Responder, Error> {
    let id = path.into_inner();

    let proxy =
        proxies
            .0
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| ServiceError::BadRequest {
                message: "bad id".into(),
            })?;

    let request = client
        .get(proxy.url)
        .no_decompress()
        .send()
        .await
        .map_err(|e| {
            log::error!("remote request creation failed: {}", e);

            ServiceError::BadRequest {
                message: "request failed".into(),
            }
        })?;

    let mut response = HttpResponse::build(request.status());

    for (k, v) in request.headers() {
        // O(nm), though there are only a few ignored headers now, so overhead is not big
        if !IGNORED_HEADERS.contains(k) {
            response.insert_header((k, v.clone()));
        }
    }

    // TODO: stop if body too large?
    Ok(response.streaming(request))
}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
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

    let proxies = web::Data::new(Proxies(Mutex::new(HashMap::new())));

    // avoid move to server
    let cloned_proxies = proxies.clone();

    let server = HttpServer::new(move || {
        let logger = middleware::Logger::default();

        let json_config = web::JsonConfig::default()
            .limit(4096)
            .error_handler(|e, _rq| {
                log::debug!("json: {}", e);

                ServiceError::BadRequest {
                    message: e.to_string(),
                }
                .into()
            });

        let path_config = web::PathConfig::default().error_handler(|e, _rq| {
            log::debug!("path: {}", e);

            ServiceError::BadRequest {
                message: e.to_string(),
            }
            .into()
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
            .app_data(cloned_proxies.clone())
            .service(web::scope("/proxy").service(post_proxy).service(get_proxy))
            .default_service(web::route().to(|| ServiceError::NotFound {}.error_response()))
    })
    .bind("0.0.0.0:8000")?
    .run();

    actix_rt::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(60));

        loop {
            interval.tick().await;

            let now = Instant::now();

            let mut proxies = proxies.0.lock().await;

            // this is O(n) which is very very very bad, need to maintain a separate
            // sorted set of valid instants probably
            proxies.retain(|_, v| v.valid_until > now);
        }
    });

    server.await
}
