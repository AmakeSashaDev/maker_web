use maker_web::{Handled, Handler, Request, Response, Server, StatusCode};
use std::str::from_utf8;
use tokio::net::TcpListener;

struct MyHandler;

impl Handler<()> for MyHandler {
    async fn handle(&self, _: &mut (), req: &Request, resp: &mut Response) -> Handled {
        let user_agent = if let Some(value) = req.header(b"user-agent") {
            format!(r#", "user_agent": {:?}"#, from_utf8(value).unwrap_or(""))
        } else {
            String::new()
        };

        let content_type = if let Some(value) = req.header(b"content-type") {
            format!(r#", "content_type": {:?}"#, from_utf8(value).unwrap_or(""))
        } else {
            String::new()
        };

        let result = format!(
            r#"{{"method": "{:?}", "path": {:?}{user_agent}{content_type}, "body": {:?}}}"#,
            req.method(),
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
