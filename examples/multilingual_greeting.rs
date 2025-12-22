use maker_web::{Handled, Handler, Request, Response, Server, StatusCode};
use tokio::net::TcpListener;

struct MyHandler;

impl Handler for MyHandler {
    async fn handle(&self, _: &mut (), req: &Request, resp: &mut Response) -> Handled {
        let text = match req.url().path_segments() {
            [b"api", b"en"] => r#"{"lang": "en", "text": "Hello, world!"}"#,
            [b"api", b"zh"] => r#"{"lang": "zh", "text": "你好世界！"}"#,
            [b"api", b"es"] => r#"{"lang": "es", "text": "¡Hola Mundo!"}"#,
            [b"api", b"ar"] => r#"{"lang": "ar", "text": "مرحبا بالعالم!"}"#,
            [b"api", b"pt"] => r#"{"lang": "pt", "text": "Olá, mundo!"}"#,
            [b"api", b"hi"] => r#"{"lang": "hi", "text": "हैलो वर्ल्ड!"}"#,
            [b"api", b"ru"] => r#"{"lang": "ru", "text": "Привет, мир!"}"#,

            [b"api", _] => {
                return resp
                    .status(StatusCode::NotFound)
                    .header("Content-Type", "application/json")
                    .body(r#"{"error": "Language not supported", "status": "Not Found"}"#)
            }
            _ => r#"{"supported_lang": ["en", "zh", "es", "ar", "pt", "hi", "ru"]}"#,
        };

        resp.status(StatusCode::Ok)
            .header("Content-Type", "application/json")
            .body(text)
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
