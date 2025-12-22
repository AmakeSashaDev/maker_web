//! maker_web - High-performance, zero-allocation HTTP server for microservices
//!
//! A performance-oriented HTTP server with comprehensive configuration
//! for memory management, connection handling, and protocol support.
//! Designed for microservices requiring fine-grained control over resources.
//!
//! # Protocol Support
//!
//! - **HTTP/1.1**: Full protocol with persistent connections and chunked encoding
//! - **HTTP/1.0**: Basic protocol support for legacy clients and simple requests
//! - **HTTP/0.9+**: [High-performance variant with keep-alive and query support](limits::Http09Limits)
//!
//! # Features
//!
//! ## 🔒 Security & Protection
//! - **Built-in DoS/DDoS protection** - enabled by default, with no performance penalty.
//! - **Fully configurable limits and timeouts** for requests, responses, and connections.
//! - **Custom connection filtering** - implement the [`ConnectionFilter`] trait to
//!   reject unwanted connections at the TCP level.
//!
//! ## 🚀 Performance & Memory
//! - **Zero-allocation** - no memory allocations after server startup.
//! - **Pre-allocated memory for each connection** - linear and transparent scaling.
//!
//! ## 🌐 Protocol & Management
//! - **Full HTTP stack** - `HTTP/1.1`, `HTTP/1.0`, [`HTTP/0.9+`
//!   ](https://docs.rs/maker_web/latest/maker_web/limits/struct.Http09Limits.html)
//!   with keep-alive.
//! - **Automatic protocol detection for each request** - keep-alive eliminates
//!   the need for manual protocol selection.
//! - **Storing data between requests** - ability to store data between requests in a
//!   single connection using the [`ConnectionData`] trait.
//!
//! ## 🏭 Production Ready
//! - **Graceful performance degradation** - automatic 503 responses when overloaded.
//! - [**Custom error format**
//!   ](https://docs.rs/maker_web/latest/maker_web/limits/struct.ServerLimits.html#structfield.json_errors) -
//!   structured JSON (with codes/descriptions) or a plain HTTP response.
//! - **Resource protection** - automatic closure of connections exceeding set limits.
//!
//! # Quick Start
//!
//! ## 1. Installation
//!
//! Add `maker_web` and [`tokio`](https://crates.io/crates/tokio) to your `Cargo.toml`:
//!
//! ```bash
//! cargo add maker_web tokio --features tokio/full
//! ```
//! Or manually:
//! ```toml
//! [dependencies]
//! maker_web = "0.1"
//! tokio = { version = "1", features = ["full"] }
//! ```
//!
//! ## 2. Usage example
//! ```no_run
//! use maker_web::{Server, Handler, Request, Response, Handled, StatusCode};
//! use tokio::net::TcpListener;
//!
//! struct MyHandler;
//!
//! impl Handler for MyHandler {
//!     async fn handle(&self, _: &mut (), _: &Request, resp: &mut Response) -> Handled {
//!         resp.status(StatusCode::Ok).body("Hello World!")
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     Server::builder()
//!         .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
//!         .handler(MyHandler)
//!         .build()
//!         .launch()
//!         .await;
//! }
//! ```
//!
//! For more examples including connection filtering and advanced configuration,
//! see the crate documentation and
//! [`examples/`](https://github.com/AmakeSashaDev/maker_web/tree/main/examples)
//! directory.
//!
//! # Use Cases
//!
//! - **High-throughput microservices** - configurable for specific workloads
//! - **Resource-constrained environments** - predictable memory usage
//! - **Internal APIs** - security-conscious defaults
//! - **Performance-critical applications** - zero-allocation design
//! - **Legacy system integration** - HTTP/1.0 compatibility
//!
//! # 🌐 Beyond the Documentation
//! For live statistics, deeper insights, and ongoing project thoughts,
//! visit the [project website](https://amakesashadev.github.io/maker_web/).
pub(crate) mod http {
    pub mod query;
    pub(crate) mod request;
    pub(crate) mod response;
    pub(crate) mod types;
}
pub(crate) mod server {
    pub(crate) mod connection;
    pub(crate) mod server_impl;
}
pub(crate) mod errors;
pub mod limits;

pub use crate::{
    http::{
        query,
        request::Request,
        response::{
            write::{BodyWriter, WriteBuffer},
            Handled, Response,
        },
        types::{Method, StatusCode, Url, Version},
    },
    server::{
        connection::{ConnectionData, ConnectionFilter},
        server_impl::{Handler, Server, ServerBuilder},
    },
};

#[doc(hidden)]
pub fn run_test<F: FnOnce(&Request, &mut Response) -> Handled>(f: F) {
    f(
        &Request::new(&crate::limits::ReqLimits::default()),
        &mut Response::new(&crate::limits::RespLimits::default()),
    );
}

#[doc(hidden)]
#[macro_export]
macro_rules! impt_default_handler {
    ($name:ident) => {
        use maker_web::{Handled, Handler, Request, Response, StatusCode};
        struct $name;

        // `<()>` to check functionality
        impl Handler<()> for $name {
            async fn handle(&self, _: &mut (), _: &Request, resp: &mut Response) -> Handled {
                resp.status(StatusCode::Ok).body("Hello world!")
            }
        }
    };
}

#[doc(hidden)]
#[cfg(test)]
pub(crate) mod tools {
    use std::str::from_utf8;

    #[inline]
    pub(crate) fn str(value: Option<&[u8]>) -> Option<&str> {
        Some(from_utf8(value?).unwrap())
    }

    #[inline]
    pub(crate) fn str_op(value: &[u8]) -> &str {
        from_utf8(value).unwrap()
    }

    #[inline]
    pub(crate) fn str_2<'a>(value: (&'a [u8], &'a [u8])) -> (&'a str, &'a str) {
        (from_utf8(value.0).unwrap(), from_utf8(value.1).unwrap())
    }
}
