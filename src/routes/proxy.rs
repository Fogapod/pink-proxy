mod constants;
mod types;

use std::collections::HashMap;

use tokio::{
    sync::Mutex,
    time::{self, Duration, Instant},
};

use actix_web::{get, post};
use actix_web::{web, Error, HttpRequest, HttpResponse, Responder};
use uuid::Uuid;

use crate::auth::authorize;
use crate::errors::ServiceError;

use self::constants::*;
use self::types::*;

#[post("")]
async fn post_proxy(
    rq: HttpRequest,
    data: web::Json<ProxyURL>,
    proxies: web::Data<Proxies>,
) -> Result<impl Responder, ServiceError> {
    authorize(&rq)?;

    let ProxyURL { url, ttl } = data.into_inner();

    if !(MIN_TTL..=MAX_TTL).contains(&ttl) {
        return Err(ServiceError::BadRequest {
            message: "ttl should be between 60 and 3600".into(),
        });
    }

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

    Ok(HttpResponse::Ok().json(ProxyID { id }))
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

pub fn config(cfg: &mut web::ServiceConfig) {
    let proxies = web::Data::new(Proxies(Mutex::new(HashMap::new())));

    cfg.app_data(web::Data::new(
        awc::ClientBuilder::new()
            .disable_redirects()
            .wrap(awc::middleware::Redirect::new().max_redirect_times(10))
            .finish(),
    ))
    .app_data(proxies.clone())
    .service(post_proxy)
    .service(get_proxy);

    // proxy cleanup task
    actix_rt::spawn(async move {
        let duration = Duration::from_secs(MIN_TTL);

        loop {
            time::sleep(duration).await;

            let now = Instant::now();

            proxies.0.lock().await.retain(
                // this is O(n) which is very very very bad, need to maintain a separate
                // sorted set of valid instants probably
                |_, v| v.valid_until > now,
            );
        }
    });
}
