# maker_web

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://github.com/AmakeSashaDev/maker_web/blob/main/LICENSE-MIT)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](https://github.com/AmakeSashaDev/maker_web/blob/main/LICENSE-APACHE)
[![Downloads](https://img.shields.io/crates/d/maker_web)](https://crates.io/crates/maker_web)
[![Crates.io](https://img.shields.io/crates/v/maker_web)](https://crates.io/crates/maker_web)
[![Documentation](https://img.shields.io/badge/docs-docs.rs-blue)](https://docs.rs/maker_web)

## Features

- **⚡ High Performance** - Zero-allocation design, pre-calculated buffers
- **🔧 Fine-grained Control** - Precise memory, connection, and protocol configuration
- **🔄 Multi-protocol** - HTTP/1.1, HTTP/1.0, and HTTP/0.9+ support
- **🎯 Async Ready** - Built on Tokio for scalable I/O
- **📊 Predictable Memory** - Pre-allocated memory per connection

## Quick Start

Add to `Cargo.toml`:

```toml
[dependencies]
maker_web = "0.1"
tokio = { version = "1", features = ["full"] }
```

## Basic example
```rust
use maker_web::{Handled, Handler, Request, Response, Server, StatusCode};
use tokio::net::TcpListener;

struct MyHandler;

impl Handler<()> for MyHandler {
    async fn handle(&self, _: &mut (), req: &Request, resp: &mut Response) -> Handled {
        match req.url().path_segments() {
            [b"api", user, b"name"] => {
                resp.status(StatusCode::Ok).body(user)
            }
            [b"api", user, b"name", b"len"] => {
                resp.status(StatusCode::Ok).body(user.len())
            }
            [b"api", b"echo", text] => {
                resp.status(StatusCode::Ok).body(text)
            }
            _ => resp.status(StatusCode::NotFound).body("qwe"),
        }
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
```

## Examples

Check the [examples directory](https://github.com/AmakeSashaDev/maker_web/blob/main/examples) for comprehensive usage examples.

## Support the author
Visit the [support page](https://github.com/AmakeSashaDev/maker_web/blob/main/docs/index.html) to learn how you can support this project.

## License

`maker_web` is licensed under either of the following, at your option:

* [MIT License](https://github.com/AmakeSashaDev/maker_web/blob/main/LICENSE-MIT)
* [Apache License 2.0](https://github.com/AmakeSashaDev/maker_web/blob/main/LICENSE-APACHE)
