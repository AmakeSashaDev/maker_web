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
//! # Performance Characteristics
//!
//! - **Zero-allocation pipeline** - no heap allocations during request/response processing
//! - **Async/await ready** - built on Tokio for scalable I/O and high concurrency  
//! - **Pre-calculated buffers** - fixed memory allocation per connection based on configured limits
//! - **Connection reuse** - efficient keep-alive and connection pooling
//! - **Configurable timeouts** - precise control over connection lifetimes and I/O operations
//! - **Multi-protocol optimization** - HTTP/1.1, HTTP/1.0, and HTTP/0.9+ for various performance needs
//!
//! # Examples
//!
//! Quick start:
//! ```no_run
//! use maker_web::{Server, Handler, Request, Response, Handled, StatusCode};
//! use tokio::net::TcpListener;
//!
//! struct MyHandler;
//!
//! impl Handler<()> for MyHandler {
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
//! Something in between :) :
//! ```no_run
//! use maker_web::{Handled, Handler, Request, Response, Server, StatusCode};
//! use tokio::net::TcpListener;
//!
//! struct MyHandler;
//!
//! impl Handler<()> for MyHandler {
//!     async fn handle(&self, _: &mut (), req: &Request, resp: &mut Response) -> Handled {
//!         match req.url().path_segments() {
//!             [b"api", user, b"name"] => {
//!                 resp.status(StatusCode::Ok).body(user)
//!             }
//!             [b"api", user, b"name", b"len"] => {
//!                 resp.status(StatusCode::Ok).body(user.len())
//!             }
//!             [b"api", b"echo", text] => {
//!                 resp.status(StatusCode::Ok).body(text)
//!             }
//!             _ => resp.status(StatusCode::NotFound).body("qwe"),
//!         }
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
//! Advanced configuration:
//! ```no_run
//! # maker_web::impt_default_handler!{MyHandler}
//! use maker_web::{Server, limits::{ConnLimits, ReqLimits, ServerLimits}};
//! use tokio::net::TcpListener;
//! use std::time::Duration;
//!
//! #[tokio::main]
//! async fn main() {
//!     Server::builder()
//!         .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
//!         .handler(MyHandler)
//!         .server_limits(ServerLimits {
//!             max_connections: 5000, // Higher concurrency
//!             ..ServerLimits::default()
//!         })
//!         .connection_limits(ConnLimits {
//!             socket_read_timeout: Duration::from_secs(5),
//!             max_requests_per_connection: 10_000,
//!             ..ConnLimits::default()
//!         })
//!         .request_limits(ReqLimits {
//!             header_count: 18,      // More headers for complex APIs
//!             body_size: 16 * 1024,  // 16KB for larger payloads
//!             ..ReqLimits::default()
//!         })
//!         .build()
//!         .launch()
//!         .await;
//! }
//! ```
//!
//! # Use Cases
//!
//! - **High-throughput microservices** - configurable for specific workloads
//! - **Resource-constrained environments** - predictable memory usage
//! - **Internal APIs** - security-conscious defaults
//! - **Performance-critical applications** - zero-allocation design
//! - **Legacy system integration** - HTTP/1.0 compatibility

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
        connection::ConnectionData,
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

        impl Handler<()> for $name {
            async fn handle(&self, _: &mut (), _: &Request, resp: &mut Response) -> Handled {
                resp.status(StatusCode::Ok).body("Hello world!")
            }
        }
    };
}

#[cfg(test)]
pub mod tools {
    use std::str::from_utf8;

    #[inline]
    pub fn str(value: Option<&[u8]>) -> Option<&str> {
        Some(from_utf8(value?).unwrap())
    }

    #[inline]
    pub fn str_op(value: &[u8]) -> &str {
        from_utf8(value).unwrap()
    }

    #[inline]
    pub fn str_2<'a>(value: (&'a [u8], &'a [u8])) -> (&'a str, &'a str) {
        (from_utf8(value.0).unwrap(), from_utf8(value.1).unwrap())
    }
}
