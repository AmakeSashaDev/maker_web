<div align="center">
  <h1>maker_web</h1>
  <h3>High-performance, zero-allocation HTTP server for microservices</h3>
</div>

[![Downloads](https://img.shields.io/crates/d/maker_web)](https://crates.io/crates/maker_web)
[![Crates.io](https://img.shields.io/crates/v/maker_web?label=version)](https://crates.io/crates/maker_web)
[![Documentation](https://img.shields.io/badge/docs-docs.rs-blue)](https://docs.rs/maker_web/latest/maker_web/)
[![Build Status](https://github.com/AmakeSashaDev/maker_web/actions/workflows/ci.yml/badge.svg)](https://github.com/AmakeSashaDev/maker_web/actions)

# Key Features

## Security & Protection
- Built-in DoS/DDoS protection — active by default, zero performance cost
- Fully configurable limits & timeouts for requests, responses, and connections

## Zero-Allocation Performance  
- Zero runtime allocations — predictable and consistent performance
- Pre-allocated per-connection memory — linear and transparent scaling

## Protocol & Control
- Full HTTP stack (1.1, 1.0, [0.9+](https://docs.rs/maker_web/latest/maker_web/limits/struct.Http09Limits.html)) with keep‑alive
- Auto‑detection per request — no manual protocol selection needed с keep-alive
- Fine-grained buffer control for each protocol

## Production Ready
- Graceful degradation — automatic 503 responses during overload
- [Configurable error format](https://docs.rs/maker_web/latest/maker_web/limits/struct.ServerLimits.html#structfield.json_errors) — structured JSON (with codes/descriptions) or plain HTTP response
- Resource protection — auto-close connections exceeding configured limits

# Benchmarks

Performance comparisons are available in the [benchmarks directory](https://github.com/AmakeSashaDev/maker_web/tree/main/benches).

# Installation

Add `maker_web` and [`tokio`](https://crates.io/crates/tokio) to your `Cargo.toml`:

```bash
cargo add maker_web tokio --features tokio/full
```
Or manually:
```toml
[dependencies]
maker_web = "0.1"
tokio = { version = "1", features = ["full"] }
```
# Usage example
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
Visit the [support page](https://amakesashadev.github.io/maker_web/) to learn how you can support this project.

## License

`maker_web` is licensed under either of the following, at your option:

* [MIT License](https://github.com/AmakeSashaDev/maker_web/blob/main/LICENSE-MIT)
* [Apache License 2.0](https://github.com/AmakeSashaDev/maker_web/blob/main/LICENSE-APACHE)
