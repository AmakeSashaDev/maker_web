<div align="center">
  <h1>maker_web</h1>
  <h3>Security-first, high-performance, zero-allocation HTTP server for microservices</h3>
</div>

[![Downloads](https://img.shields.io/crates/d/maker_web)](https://crates.io/crates/maker_web)
[![Crates.io](https://img.shields.io/crates/v/maker_web?label=version)](https://crates.io/crates/maker_web)
[![Documentation](https://img.shields.io/badge/docs-docs.rs-blue)](https://docs.rs/maker_web/latest/maker_web/)
[![Build Status](https://github.com/AmakeSashaDev/maker_web/actions/workflows/ci.yml/badge.svg)](https://github.com/AmakeSashaDev/maker_web/actions)
[![GitCode](https://img.shields.io/badge/GitCode-Mirror-FF6600?logo=data:image/png;data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAGAAAABgCAMAAADVRocKAAAAPFBMVEUAAADbIEDaID7aID/bID7fIEDaID7bIEDaID7aID7bID7cIEDbID3aID/aID/aID/YID3aID7dID7aID5zWHPKAAAAE3RSTlMAIN+/7xCfQGCAkDBwz69QcM9vGOdDSAAAArBJREFUaN7tmdtS7SAMhoEknArtUt7/XbfjdmbrsiQE2gtn+905o3+aIwHN/4d9w9yBPYqvrn3gYE+BrhMv3rUTqj+uUE/Q+qAPS+oUoUm4aKflE7YR0Ns1eRlM+owfrmlwUZlaaFp2TZxesenBx3D0c5vD01h4apul0oi+a/Pssv6GbYV4s36rUnywLRKk+K/C1ipp9fP2vVQ5A14nD9YY1Bh41U2f92i/fHOKScBzTXMBg9hxmjnknvXIhK9HJTr3/rPby2Y+KM8G+iMvnv5qSFBbwzfN80M+DKfAujMDMvA1MVZRQduQgTCYAdvpSZn0KUlR1QLRKC2A5Up03oCx0QP4wuYs8mNrnbOeshfqh7NRoBXRTjn/0yN0qQPpZgcM3OyAuduB7W4HDnUXU0zewwc+pxI2UuXYserntzaEXMJom/nZiw/ukS8iOUJ14q7z0p4hNpyyCWlQAD9UZJLwR1moaJmd2D57SFNLBlgPwoQHXBTc+MJCOGzhYAxYuSllHPX7wDDkYQup38nC+85ehwKF/Y8iI0HhKMkDsG0R+nu1+hlvC2XvF9LWrSKdId+byaRZGjUF0J12WSHKdYntWa4XLXC2O2Homv3HdgdAuiYJ/V7GSQMvPZWwVkedegdmSCJdsCE+OtmZrtRXZiCQ+oYm3/OA341w0+o79hNt4y3o9Z18kDwU+gE7w5o9bTMp8itun4XZ0fQv3WjZKv4HBMVLtBBeizP/4Qj7+PNpaR2ghN4jxflHodWvJJA/XWPIHiUD6h+Ya5NA5xw2gSx2y/L79a0WnJU6fl2ft7Cuf6cFZ9WNr9eXSbP6MDod41yYknqR1b73M6w7kUm9B2pMQJj6Z7hTyuuJIKvjHswCNlf+4wuZVWz050ZcjmQugkJJO1SHf88E8DluZH75CfwBt54WqcTQWY8AAAAASUVORK5CYII=)](https://gitcode.com/AmakeSashaDev/maker_web)

# ✨ Features

## 🔒 Security & Protection
- **Built-in DoS/DDoS protection** - enabled by default, with no performance penalty.
- **Fully configurable limits and timeouts** for requests, responses, and connections.
- **Custom connection filtering** - implement the [`ConnectionFilter`](https://docs.rs/maker_web/latest/maker_web/trait.ConnectionFilter.html) trait to
reject unwanted connections at the TCP level.

## ⚡ Performance & Memory
- **Zero-allocation** - no memory allocations after server startup.
- **Pre-allocated memory for each connection** - linear and transparent scaling.

## 🌐 Protocol & Management
- **Full HTTP stack** - `HTTP/1.1`, `HTTP/1.0`, [`HTTP/0.9+`
](https://docs.rs/maker_web/latest/maker_web/limits/struct.Http09Limits.html)
with keep-alive.
- **Automatic protocol detection for each request** - keep-alive eliminates
the need for manual protocol selection.
- **Storing data between requests** - ability to store data between requests in a
single connection using the [`ConnectionData`] trait.

## 🏭 Production Ready
- **Graceful performance degradation** - automatic 503 responses when overloaded.
- [**Custom error format**
](https://docs.rs/maker_web/latest/maker_web/limits/struct.ServerLimits.html#structfield.json_errors) -
structured JSON (with codes/descriptions) or a plain HTTP response.
- **Resource protection** - automatic closure of connections exceeding set limits.

# 🎯 Use Cases

- **High-throughput microservices** - configurable for specific workloads
- **Resource-constrained environments** - predictable memory usage  
- **Internal APIs** - security-conscious defaults
- **Performance-critical applications** - zero-allocation design
- **Legacy system integration** - HTTP/1.0 compatibility

# 🌐 Not just code
Everything that remains outside the documentation—live statistics, deep details, and informal plans—I collect on a [separate website](https://amakesashadev.github.io/maker_web/). This is a space that I strive to keep current and meaningful.

# 🚀 Quick Start

## 1. Installation

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
## 2. Usage example
```rust
use maker_web::{Handled, Handler, Request, Response, Server, StatusCode};
use tokio::net::TcpListener;

struct MyHandler;

impl Handler for MyHandler {
    async fn handle(&self, _: &mut (), req: &Request, resp: &mut Response) -> Handled {
        match req.url().path_segments_str() {
            ["api", user, "name"] => {
                resp.status(StatusCode::Ok).body(user)
            }
            ["api", user, "name", "len"] => {
                resp.status(StatusCode::Ok).body(user.len())
            }
            ["api", "echo", text] => {
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

# 📖 Examples

Check the [examples directory](https://github.com/AmakeSashaDev/maker_web/blob/main/examples) for comprehensive usage examples.

# 📊 Benchmarks

Performance comparisons are available in the [benchmarks directory](https://github.com/AmakeSashaDev/maker_web/tree/main/benches).

# 📄 License

`maker_web` is licensed under either of the following, at your option:

* [MIT License](https://github.com/AmakeSashaDev/maker_web/blob/main/LICENSE-MIT)
* [Apache License 2.0](https://github.com/AmakeSashaDev/maker_web/blob/main/LICENSE-APACHE)
