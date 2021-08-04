mod errors;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::{sync::Mutex, time};

use actix_web::{get, post};
use actix_web::{middleware, web, App, Error, HttpResponse, HttpServer, Responder, ResponseError};

use serde::{Deserialize, Serialize};

use uuid::Uuid;

use errors::ServiceError;

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
struct Proxies(Mutex<HashMap<Uuid, Proxy>>);

#[post("")]
async fn post_proxy(
    data: web::Json<ProxyURL>,
    proxies: web::Data<Proxies>,
) -> Result<impl Responder, Error> {
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

    let request = client.get(proxy.url).send().await.map_err(|e| {
        log::error!("remote request creation failed: {}", e);

        ServiceError::BadRequest {
            message: "request failed".into(),
        }
    })?;

    let mut response = HttpResponse::build(request.status());

    for (k, v) in request.headers() {
        // O(nm), though there are only 2 ignored headers now, so overhead is not big
        if !IGNORED_HEADERS.contains(k) {
            response.insert_header((k, v.clone()));
        }
    }

    // TODO: stop if body too large?
    Ok(response.streaming(request))
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

    // proxy cleanup task
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
