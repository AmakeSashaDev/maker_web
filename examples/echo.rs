use maker_web::{Handled, Handler, Request, Response, Server, StatusCode};
use std::str::from_utf8;
use tokio::net::TcpListener;

struct MyHandler;

impl Handler for MyHandler {
    async fn handle(&self, _: &mut (), req: &Request, resp: &mut Response) -> Handled {
        let result = format!(
            r#"{{"url": {:?}, "body": {:?}}}"#,
            from_utf8(req.url().path()).unwrap_or(""),
            from_utf8(req.body().unwrap_or(&[])).unwrap_or(""),
        );

        resp.status(StatusCode::Ok)
            .header("Content-Type", "application/json")
            .body(result)
    }
}

#[tokio::main]
async fn main() {
    Server::builder()
        .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
        .handler(MyHandler)
        .build()
        .launch()
        .await;
}
