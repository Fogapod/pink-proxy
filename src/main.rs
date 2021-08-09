mod auth;
mod errors;
mod routes;

#[cfg(not(feature = "error_reporting"))]
mod dummy_sentry;
#[cfg(not(feature = "error_reporting"))]
use dummy_sentry as sentry_actix;

use actix_web::{middleware, web, App, HttpServer, ResponseError};

use errors::ServiceError;

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    std::env::set_var("RUST_BACKTRACE", "1");

    dotenv::dotenv().ok();

    env_logger::init();

    #[cfg(feature = "error_reporting")]
    let _guard = sentry::init((
        std::env::var("SENTRY_DSN").expect("SENTRY_DSN not set"),
        sentry::ClientOptions {
            release: sentry::release_name!(),
            ..Default::default()
        },
    ));

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
            .wrap(logger)
            .wrap(middleware::Condition::new(
                cfg!(feature = "error_reporting"),
                sentry_actix::Sentry::new(),
            ))
            .app_data(json_config)
            .app_data(path_config)
            .configure(routes::config)
            .default_service(web::route().to(|| ServiceError::NotFound {}.error_response()))
    })
    .bind("0.0.0.0:8000")?
    .run();

    server.await
}
