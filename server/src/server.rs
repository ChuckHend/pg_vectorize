use actix_web::web;

use crate::routes;

pub fn route_config(configuration: &mut web::ServiceConfig) {
    configuration.service(web::scope("/api/v1").service(routes::table::table));
}
