use actix_web::web;

use crate::routes;
use vectorize_core::worker::base::Config;

use sqlx::{Pool, Postgres};

pub fn route_config(configuration: &mut web::ServiceConfig) {
    configuration.service(web::scope("/api/v1").service(routes::table::table));
}
