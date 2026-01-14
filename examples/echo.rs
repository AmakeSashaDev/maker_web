use maker_web::{Handled, Handler, Request, Response, Server, StatusCode};
use tokio::net::TcpListener;

struct MyHandler;

impl Handler for MyHandler {
    async fn handle(&self, _: &mut (), req: &Request, resp: &mut Response) -> Handled {
        let body = if let Some(body) = req.body() {
            format!(r#", "body": {:?}"#, body)
        } else {
            String::new()
        };

        let result = format!(r#"{{"url": {:?}{body}}}"#, req.url().path_str());

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
