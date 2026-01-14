use maker_web::{Handled, Handler, Request, Response, Server, StatusCode};
use tokio::net::TcpListener;

struct HelloWorld;

impl Handler for HelloWorld {
    async fn handle(&self, _: &mut (), _: &Request, resp: &mut Response) -> Handled {
        resp.status(StatusCode::Ok)
            .header("Content-Type", "text/plain")
            .body("Hello, world!")
    }
}

// Changing the basic settings is necessary due to their default conservatism.
#[tokio::main]
async fn main() {
    use maker_web::limits::{ConnLimits, ServerLimits};

    Server::builder()
        .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
        .handler(HelloWorld)
        .server_limits(ServerLimits {
            max_connections: 5000,
            ..ServerLimits::default()
        })
        .connection_limits(ConnLimits {
            max_requests_per_connection: 100000,
            ..ConnLimits::default()
        })
        .build()
        .launch()
        .await;
}
