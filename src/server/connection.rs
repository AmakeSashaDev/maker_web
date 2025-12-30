use crate::{
    errors::ErrorKind,
    http::{
        request::{Parser, Request},
        response::Response,
        types::Version,
    },
    limits::{ConnLimits, Http09Limits, ReqLimits, RespLimits, ServerLimits},
    server::server_impl::{AllLimits, Handler},
    Handled,
};
use std::{future::Future, io, net::SocketAddr, sync::Arc, time::Instant};
use tokio::{io::AsyncWriteExt, net::TcpStream, time::sleep};

pub(crate) struct HttpConnection<H: Handler<S>, S: ConnectionData> {
    handler: Arc<H>,
    connection_data: S,

    connection: Connection,
    pub(crate) parser: Parser,
    pub(crate) request: Request,
    pub(crate) response: Response,

    pub(crate) server_limits: ServerLimits,
    pub(crate) conn_limits: ConnLimits,
    pub(crate) http_09_limits: Option<Http09Limits>,
    pub(crate) req_limits: ReqLimits,
    pub(crate) resp_limits: RespLimits,
}

impl<H: Handler<S>, S: ConnectionData> HttpConnection<H, S> {
    #[inline]
    pub(crate) fn new(handler: Arc<H>, limits: AllLimits) -> Self {
        Self {
            handler,
            connection_data: S::new(),

            connection: Connection::new(),
            parser: Parser::new(&limits.3),
            request: Request::new(&limits.3),
            response: Response::new(&limits.4),

            server_limits: limits.0,
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
    pub(crate) async fn run(
        &mut self,
        stream: &mut TcpStream,
        client_addr: SocketAddr,
        server_addr: SocketAddr,
    ) -> Result<(), io::Error> {
        self.request.client_addr = client_addr;
        self.request.server_addr = server_addr;

        match self.impl_run(stream).await {
            Ok(()) => Ok(()),
            Err(ErrorKind::Io(e)) => Err(e.0),
            Err(error) => {
                self.conn_limits
                    .send_error(
                        stream,
                        error,
                        self.request.version(),
                        self.server_limits.json_errors,
                    )
                    .await
            }
        }
    }

    #[inline]
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

            self.conn_limits
                .write_bytes(stream, self.response.buffer())
                .await?;

            if !self.response.keep_alive {
                break;
            }

            self.connection.request_count += 1;
        }

        Ok(())
    }
}

impl ConnLimits {
    #[inline]
    pub(crate) async fn send_error(
        &self,
        stream: &mut TcpStream,
        error: ErrorKind,
        version: Version,
        json_errors: bool,
    ) -> Result<(), io::Error> {
        self.write_bytes(stream, error.as_http(version, json_errors))
            .await
    }

    #[inline]
    pub(crate) async fn write_bytes(
        &self,
        stream: &mut TcpStream,
        response: &[u8],
    ) -> Result<(), io::Error> {
        tokio::select! {
            biased;

            result = stream.write_all(response) => result,
            _ = sleep(self.socket_write_timeout) => {
                Err(io::Error::new(io::ErrorKind::TimedOut, "write timeout"))
            },
        }
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
    #[inline]
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
    #[inline]
    pub(crate) fn new() -> Self {
        Self {
            created: Instant::now(),
            request_count: 0,
        }
    }

    #[inline]
    pub(crate) fn reset(&mut self) {
        self.created = Instant::now();
        self.request_count = 0;
    }
}

//

/// Managing user session data stored between requests within a single HTTP connection.
///
/// This trait allows you to store arbitrary state (e.g., authentication data,
/// multistep form status, cache, etc.). The state is available across all requests
/// within a single HTTP keep-alive connection.
///
/// # Examples
/// ```no_run
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
///
/// Check out a [real-world example
/// ](https://github.com/AmakeSashaDev/maker_web/blob/main/examples/request_counter.rs)
/// (well, almost)
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
    fn new() -> Self {}

    #[inline(always)]
    fn reset(&mut self) {}
}

/// A trait for filtering TCP connections before HTTP processing.
///
/// # Examples
///
/// Simple IP Blacklist:
/// ```
/// use std::{collections::HashSet, net::{SocketAddr, IpAddr}};
/// use maker_web::{Server, ConnectionFilter, Response, Handled, StatusCode};
///
/// struct MyConnFilter {
///     blacklist: HashSet<IpAddr>
/// }
///
/// impl ConnectionFilter for MyConnFilter {
///     fn filter(
///         &self, client_addr: SocketAddr, _: SocketAddr, err_resp: &mut Response
///     ) -> Result<(), Handled> {
///         if self.blacklist.contains(&client_addr.ip()) {
///             Err(err_resp
///                 .status(StatusCode::Forbidden)
///                 .body("Your IP is permanently banned"))
///         } else {
///             Ok(())
///         }
///     }
/// }
/// ```
/// File-based IP blacklist:
/// ```
/// use std::net::SocketAddr;
/// use maker_web::{Server, ConnectionFilter, Response, Handled, StatusCode};
///
/// # struct DatabaseClient;
/// #
/// # impl DatabaseClient {
/// #     async fn execute(&self, _: &str) -> Option<Vec<&str>> {
/// #         Some(vec!["true"])
/// #     }
/// # }
/// #
/// #
/// struct MyConnFilter {
///     db: DatabaseClient
/// }
///
/// impl ConnectionFilter for MyConnFilter {
///     fn filter(&self, _: SocketAddr, _: SocketAddr, _: &mut Response) -> Result<(), Handled> {
///         Ok(())
///     }
///
///     async fn filter_async(
///         &self,
///         client_addr: SocketAddr,
///         _: SocketAddr,
///         err_resp: &mut Response,
///     ) -> Result<(), Handled> {
///         let request = format!(
///             "SELECT EXISTS (SELECT 1 FROM ip_blacklist WHERE ip_address = '{}')",
///             client_addr.ip()
///         );
///
///         if self.db.execute(&request).await == Some(vec!["false"]) {
///             Ok(()) // IP not found in blacklist
///         } else {
///             Err(err_resp
///                 .status(StatusCode::Forbidden)
///                 .body("IP found in blacklist file"))
///         }
///     }
/// }
/// ```
/// Two-stage filtering with cache:
/// ```
/// use std::{collections::HashSet, sync::RwLock, net::{SocketAddr, IpAddr}};
/// use maker_web::{Server, ConnectionFilter, Response, Handled, StatusCode};
///
/// # struct DatabaseClient;
/// #
/// # impl DatabaseClient {
/// #     async fn execute(&self, _: &str) -> Option<Vec<&str>> {
/// #         Some(vec!["true"])
/// #     }
/// # }
/// #
/// #
/// struct MyConnFilter {
///     cache: RwLock<HashSet<IpAddr>>,
///     db: DatabaseClient,
/// }
///
/// impl ConnectionFilter for MyConnFilter {
///     fn filter(
///         &self, client_addr: SocketAddr, _: SocketAddr, err_resp: &mut Response
///     ) -> Result<(), Handled> {
///         let Ok(guard) = self.cache.read() else {
///             return Err(err_resp.status(StatusCode::InternalServerError)
///                 .body("Internal server error"));
///         };
///
///         if guard.contains(&client_addr.ip()) {
///             Err(err_resp
///                 .status(StatusCode::Forbidden)
///                 .body("Your IP is permanently banned"))
///         } else {
///             Ok(())
///         }
///     }
///
///     async fn filter_async(
///         &self,
///         client_addr: SocketAddr,
///         _: SocketAddr,
///         err_resp: &mut Response,
///     ) -> Result<(), Handled> {
///         let request = format!(
///             "SELECT EXISTS (SELECT 1 FROM ip_blacklist WHERE ip_address = '{}')",
///             client_addr.ip()
///         );
///
///         if self.db.execute(&request).await == Some(vec!["false"]) {
///             Ok(()) // IP not found in blacklist
///         } else {
///             let Ok(mut guard) = self.cache.write() else {
///                 return Err(err_resp.status(StatusCode::InternalServerError)
///                     .body("Internal server error"));
///             };
///             guard.insert(client_addr.ip());
///
///             Err(err_resp
///                 .status(StatusCode::Forbidden)
///                 .body("IP found in blacklist file"))
///         }
///     }
/// }
/// ```
/// # Connection Filter Architecture
/// ```text
///                     [ QUEUE TCP_STREAM ]
///                              ||
/// /----------------------------||----------------------------------\
/// |                            || TCP_STREAM            Tokio Task |
/// |       /=====================/                                  |
/// |       \/                                                       |
/// |   [--------]   Err(Handled)   [----------------------]         |
/// |   [ filter ] ===============> [ Send `error_response`]         |
/// |   [--------]                  [----------------------]         |
/// |       ||                                 /\                    |
/// |       || Ok(())                          ||                    |
/// |       \/                Err(Handled)     ||                    |
/// |   [--------------] ========================/                   |
/// |   [ filter_async ]                             [-----------]   |
/// |   [--------------] ==========================> [  Handler  ]   |
/// |                             Ok(())             [-----------]   |
/// |                                                                |
/// \----------------------------------------------------------------/
/// ```
pub trait ConnectionFilter: Sync + Send + 'static {
    /// Synchronous connection validation.
    ///
    /// Perform fast, in-memory checks here. Expensive operations should be deferred
    /// to [`filter_async`](Self::filter_async).
    ///
    /// Use for:
    /// - IP blacklist/whitelist (in-memory cache)
    /// - Geographic IP restrictions
    /// - Rate limiting counters
    fn filter(
        &self,
        client_addr: SocketAddr,
        server_addr: SocketAddr,
        error_response: &mut Response,
    ) -> Result<(), Handled>;

    /// Asynchronous connection inspection.
    ///
    /// Called after [`filter`](Self::filter) succeeds.Executes asynchronously within
    /// the Tokio runtime.
    ///
    /// Use for:
    /// - Database lookups
    /// - External API calls
    /// - File system operations
    /// - Complex business logic
    /// - Machine learning inference
    fn filter_async(
        &self,
        #[allow(unused_variables)] client_addr: SocketAddr,
        #[allow(unused_variables)] server_addr: SocketAddr,
        #[allow(unused_variables)] error_response: &mut Response,
    ) -> impl Future<Output = Result<(), Handled>> + Send {
        async { Ok(()) }
    }
}

impl ConnectionFilter for () {
    fn filter(&self, _: SocketAddr, _: SocketAddr, _: &mut Response) -> Result<(), Handled> {
        Ok(())
    }
}

//

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
        #[inline]
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

                server_limits: ServerLimits::default(),
                conn_limits: ConnLimits::default(),
                http_09_limits: None,
                req_limits,
                resp_limits,
            }
        }
    }
}
