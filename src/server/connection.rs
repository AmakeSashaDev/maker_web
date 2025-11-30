use crate::{
    errors::ErrorKind,
    http::{
        request::{Parser, Request},
        response::Response,
        types::Version,
    },
    limits::{ConnLimits, Http09Limits, ReqLimits, RespLimits},
    server::server::{AllTypesLimits, Handler},
};
use std::{io, sync::Arc, time::Instant};
use tokio::net::TcpStream;

pub(crate) struct HttpConnection<H: Handler<S>, S: ConnectionData> {
    handler: Arc<H>,
    connection_data: S,

    connection: Connection,
    pub(crate) parser: Parser,
    pub(crate) request: Request,
    pub(crate) response: Response,

    conn_limits: ConnLimits,
    pub(crate) http_09_limits: Option<Http09Limits>,
    pub(crate) req_limits: ReqLimits,
    resp_limits: RespLimits,
}

impl<H: Handler<S>, S: ConnectionData> HttpConnection<H, S> {
    #[inline]
    pub(crate) fn new(handler: Arc<H>, limits: AllTypesLimits) -> Self {
        Self {
            handler,
            connection_data: S::new(),

            connection: Connection::new(),
            parser: Parser::new(&limits.3),
            request: Request::new(&limits.3),
            response: Response::new(&limits.4),

            conn_limits: limits.1,
            http_09_limits: limits.2,
            req_limits: limits.3,
            resp_limits: limits.4,
        }
    }

    #[inline]
    fn reset_request_response(&mut self) {
        self.parser.reset();
        self.request.reset();
        self.response.reset(&self.resp_limits);
    }
}

impl<H: Handler<S>, S: ConnectionData> HttpConnection<H, S> {
    #[inline]
    pub(crate) async fn run(&mut self, stream: &mut TcpStream) -> Result<(), io::Error> {
        match self.impl_run(stream).await {
            Ok(()) => Ok(()),
            Err(ErrorKind::Io(e)) => Err(e.0),
            Err(err) => writer::send_error(stream, self.request.version(), err).await,
        }
    }

    #[inline(always)]
    pub(crate) async fn impl_run(&mut self, stream: &mut TcpStream) -> Result<(), ErrorKind> {
        self.connection.reset();
        self.connection_data.reset();

        while !self.is_expired()? {
            self.reset_request_response();

            if self
                .parser
                .fill_buffer(stream, self.conn_limits.socket_read_timeout)
                .await?
                == 0
            {
                break;
            }

            self.response.version = self.parse()?;

            self.handler
                .handle(&mut self.connection_data, &self.request, &mut self.response)
                .await;

            writer::write_bytes(stream, self.response.buffer()).await?;

            if !self.response.keep_alive {
                break;
            }

            self.connection.request_count += 1;
        }

        Ok(())
    }
}

pub(crate) mod writer {
    use crate::{errors::ErrorKind, http::types::Version};
    use std::{
        io,
        sync::atomic::{AtomicBool, AtomicU64, Ordering},
    };
    use tokio::{
        io::AsyncWriteExt,
        net::TcpStream,
        time::{timeout, Duration},
    };

    pub(crate) static SOCKET_WRITE_TIMEOUT: AtomicU64 = AtomicU64::new(0);
    pub(crate) static JSON_ERRORS: AtomicBool = AtomicBool::new(true);

    #[inline(always)]
    pub(crate) async fn send_error(
        stream: &mut TcpStream,
        version: Version,
        error: ErrorKind,
    ) -> Result<(), io::Error> {
        write_bytes(
            stream,
            error.as_http(version, JSON_ERRORS.load(Ordering::Relaxed)),
        )
        .await
    }

    #[inline(always)]
    pub(crate) async fn write_bytes(
        stream: &mut TcpStream,
        response: &[u8],
    ) -> Result<(), io::Error> {
        let num = SOCKET_WRITE_TIMEOUT.load(Ordering::Relaxed);
        timeout(Duration::from_micros(num), stream.write_all(response)).await?
    }
}

macro_rules! is_expired {
    ($self:expr, $limits:expr) => {
        Ok(!$self.response.keep_alive
            || $self.connection.request_count >= $limits.max_requests_per_connection
            || $self.connection.created.elapsed() > $limits.connection_lifetime)
    };
}

impl<H: Handler<S>, S: ConnectionData> HttpConnection<H, S> {
    #[inline(always)]
    fn is_expired(&self) -> Result<bool, ErrorKind> {
        match (self.response.version, &self.http_09_limits) {
            (Version::Http09, Some(limits)) => is_expired!(self, limits),
            (Version::Http09, None) => Err(ErrorKind::UnsupportedVersion),
            _ => is_expired!(self, self.conn_limits),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Connection {
    created: Instant,
    request_count: usize,
}

impl Connection {
    #[inline(always)]
    pub(crate) fn new() -> Self {
        Self {
            created: Instant::now(),
            request_count: 0,
        }
    }

    #[inline(always)]
    pub(crate) fn reset(&mut self) {
        self.created = Instant::now();
        self.request_count = 0;
    }
}

//

/// Managing user session data stored between requests within a single HTTP connection.
///
/// This trait allows you to store an arbitrary state (for example, authentication,
/// multistep form status, cache, etc.), which will be available in all requests
/// during the lifetime of the HTTP connection (keep-alive). Data is automatically
/// reset when the connection is terminated.
///
/// # Examples
/// ```
/// use maker_web::ConnectionData;
/// use std::collections::HashMap;
///
/// struct MyConnectionData {
///     user_id: Option<i32>,
///     request_count: usize,
///     cache: HashMap<usize, [u8; 4]>,
/// }
///
/// impl ConnectionData for MyConnectionData {
///     fn new() -> Self {
///         Self {
///             user_id: None,
///             request_count: 0,
///             cache: HashMap::new(),
///         }
///     }
///
///     fn reset(&mut self) {
///         self.user_id = None;
///         self.request_count = 0;
///         self.cache.clear(); // Saving the allocated memory
///     }
/// }
/// ```
pub trait ConnectionData: Sync + Send + 'static {
    /// Creates a new instance of user data.
    ///
    /// It is called once at server startup, which avoids runtime allocations.
    fn new() -> Self;

    /// Resets the internal state of the instance to its initial values.
    ///
    /// It is called after the connection is closed. Allows repeated
    /// use of the instance for the following connections. If implemented
    /// correctly, avoids any allocations.
    fn reset(&mut self);
}

impl ConnectionData for () {
    #[inline(always)]
    fn new() -> Self {
        ()
    }

    #[inline(always)]
    fn reset(&mut self) {}
}

#[cfg(test)]
mod def_handler {
    use super::*;
    use crate::{Handled, StatusCode};

    pub(crate) struct DefHandler;

    impl Handler<()> for DefHandler {
        async fn handle(&self, _: &mut (), _: &Request, r: &mut Response) -> Handled {
            r.status(StatusCode::Ok).body("test")
        }
    }

    impl HttpConnection<DefHandler, ()> {
        pub(crate) fn from_req<V: AsRef<[u8]>>(value: V) -> Self {
            let req_limits = ReqLimits::default().precalculate();
            let resp_limits = RespLimits::default();

            Self {
                handler: Arc::new(DefHandler),
                connection_data: (),

                connection: Connection::new(),
                parser: Parser::from(&req_limits, value),
                request: Request::new(&req_limits),
                response: Response::new(&resp_limits),

                conn_limits: ConnLimits::default(),
                http_09_limits: None,
                req_limits,
                resp_limits,
            }
        }
    }
}
