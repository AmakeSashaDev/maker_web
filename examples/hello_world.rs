use maker_web::{Handled, Handler, Request, Response, Server, StatusCode};
use tokio::net::TcpListener;

struct HelloWorld;

impl Handler<()> for HelloWorld {
    async fn handle(&self, _: &mut (), _: &Request, resp: &mut Response) -> Handled {
        resp.status(StatusCode::Ok)
            .header("Content-Type", "text/plain")
            .body("Hello, world!")
    }
}

#[tokio::main]
async fn main() {
    Server::builder()
        .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
        .handler(HelloWorld)
        .build()
        .launch()
        .await;
}
