use actix_cors::Cors;
use actix_web::{App, HttpServer, middleware, web};
use std::time::Duration;

use vectorize_server::init;

use vectorize_server::core::worker::base::Config;

#[actix_web::main]
async fn main() {
    env_logger::init();

    let cfg = Config::from_env();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&cfg.database_url)
        .await
        .expect("unable to connect to postgres");
    let server_port = cfg.webserver_port;
    let server_workers = cfg.num_server_workers;
    init::init_project(&pool, Some(&cfg.database_url))
        .await
        .expect("Failed to initialize project");
    let _ = HttpServer::new(move || {
        let cors = Cors::permissive();

        App::new()
            .wrap(cors)
            .wrap(middleware::Logger::default())
            .app_data(web::Data::new(cfg.clone()))
            .app_data(web::Data::new(pool.clone()))
            .configure(vectorize_server::server::route_config)
    })
    .workers(server_workers)
    .keep_alive(Duration::from_secs(75))
    .bind(("0.0.0.0", server_port))
    .expect("Failed to bind server")
    .run()
    .await;
}
