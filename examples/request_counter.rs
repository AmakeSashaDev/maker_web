use maker_web::{ConnectionData, Handled, Handler, Request, Response, Server, StatusCode};
use tokio::net::TcpListener;

struct MyHandler;

struct Counter(usize);

impl ConnectionData for Counter {
    fn new() -> Self {
        Counter(0)
    }

    fn reset(&mut self) {
        self.0 = 0;
    }
}

impl Handler<Counter> for MyHandler {
    async fn handle(&self, counter: &mut Counter, _: &Request, resp: &mut Response) -> Handled {
        counter.0 += 1;

        resp.status(StatusCode::Ok)
            .header("Content-Type", "application/json")
            .body(format!(r#"{{"count_request": {}}}"#, counter.0))
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
