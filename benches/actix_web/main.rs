use actix_web::{web, App, HttpServer, Responder};

async fn handler() -> impl Responder {
    "Hello, world!"
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| App::new().default_service(web::route().to(handler)))
        .bind(("127.0.0.1", 8080))?
        .run()
        .await
}
