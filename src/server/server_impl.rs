use crate::{
    errors::ErrorKind,
    http::{
        request::Request,
        response::{Handled, Response},
    },
    limits::{ConnLimits, Http09Limits, ReqLimits, RespLimits, ServerLimits, WaitStrategy},
    server::connection::{ConnectionData, HttpConnection},
    ConnectionFilter, Version,
};
use crossbeam::queue::SegQueue;
use std::{
    future::Future,
    marker::{PhantomData, Send, Sync},
    net::SocketAddr,
    sync::Arc,
};
use tokio::{
    net::{TcpListener, TcpStream},
    task::yield_now,
    time::sleep as tokio_sleep,
};

/// A trait for handling HTTP requests and generating responses.
///
/// You can use:
/// - `&self` for shared immutable data (e.g. database connection pool, router configuration)
/// - `&mut S` for connection-specific mutable state (e.g. authentication tokens, session data)
///
/// # Examples
///
/// Basic Request Handler
/// ```
/// use maker_web::{Handler, Request, Response, Handled, StatusCode};
///
/// struct MyHandler;
///
/// impl Handler for MyHandler {
///     async fn handle(&self, _: &mut (), req: &Request, resp: &mut Response) -> Handled {
///         // Simple echo handler
///         if req.url().target() == b"/echo" {
///             resp.status(StatusCode::Ok).body("Echo response")
///         } else {
///             resp.status(StatusCode::NotFound).body("Not found :(")
///         }
///     }
/// }
/// ```
/// Handler with [`ConnectionData`]
/// ```
/// use maker_web::{Handler, ConnectionData, Request, Response, Handled, StatusCode};
///
/// struct CountingHandler;
///
/// impl Handler<State> for CountingHandler {
///     async fn handle(&self, data: &mut State, req: &Request, resp: &mut Response) -> Handled {
///         data.request_count += 1;
///
///         resp.status(StatusCode::Ok)
///             .body(format!("Request #{}", data.request_count))
///     }
/// }
///
/// struct State {
///     request_count: usize,
/// }
///
/// impl ConnectionData for State {
///     fn new() -> Self {
///         Self { request_count: 0 }
///     }
///     
///     fn reset(&mut self) {
///         self.request_count = 0;
///     }
/// }
/// ```
pub trait Handler<S = ()>
where
    Self: Sync + Send + 'static,
    S: ConnectionData,
{
    /// Processes an HTTP request and generates a response.
    ///
    /// # Parameters
    ///
    /// - `connection_data`: Mutable reference to connection-specific state
    /// - `req`: Immutable reference to the parsed HTTP request
    /// - `resp`: Mutable response builder for constructing the response
    ///
    /// # Returns
    ///
    /// `Handled` indicating whether the request was fully processed or
    /// requires further handling by other middleware.
    ///
    /// # Errors
    ///
    /// Implementations should handle errors internally and set appropriate
    /// HTTP status codes on the response. Panics will terminate the connection.
    fn handle(
        &self,
        connection_data: &mut S,
        request: &Request,
        response: &mut Response,
    ) -> impl Future<Output = Handled> + Send;
}

/// An HTTP server that processes incoming connections and requests.
///
/// The server uses a pre-allocated connection pool for maximum performance
/// and implements graceful connection handling with configurable limits.
///
/// # Examples
///
/// ```no_run
/// use maker_web::{Server, Handler, Request, Response, Handled, StatusCode};
/// use tokio::net::TcpListener;
///
/// struct MyHandler;
///
/// impl Handler for MyHandler {
///     async fn handle(&self, _: &mut (), _: &Request, resp: &mut Response) -> Handled {
///         resp.status(StatusCode::Ok).body("Hello world!")
///     }
/// }
///
/// #[tokio::main]
/// async fn main() {
///     Server::builder()
///         .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
///         .handler(MyHandler)
///         .build()
///         .launch()
///         .await
/// }
/// ```
pub struct Server {
    listener: TcpListener,
    stream_queue: TcpQueue,
    error_queue: TcpQueue,
    server_limits: ServerLimits,
}

impl Server {
    /// Creates a new builder for configuring the server instance.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # maker_web::impt_default_handler!{ MyStruct }
    /// # #[tokio::main]
    /// # async fn main() {
    /// use tokio::net::TcpListener;
    /// use maker_web::Server;
    ///
    /// let server = Server::builder()
    ///     .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
    ///     .handler(MyStruct) // structure with Handler implementation
    ///     .build();
    /// # }
    /// ```
    #[inline]
    pub fn builder<H, S>() -> ServerBuilder<H, S, ()>
    where
        H: Handler<S>,
        S: ConnectionData,
    {
        ServerBuilder {
            listener: None,
            handler: None,
            connection_filter: Arc::new(()),
            _marker: PhantomData,

            server_limits: None,
            request_limits: None,
            response_limits: None,
            connection_limits: None,
            http_09_limits: None,
        }
    }

    /// Starts the server and begins accepting incoming connections.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # maker_web::impt_default_handler!{ MyStruct }
    /// # #[tokio::main]
    /// # async fn main() {
    /// use maker_web::Server;
    /// use tokio::net::TcpListener;
    ///
    /// Server::builder()
    ///     .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
    ///     .handler(MyStruct) // structure with Handler implementation
    ///     .build()
    ///     .launch()
    ///     .await
    /// # }
    /// ```
    #[inline]
    pub async fn launch(self) {
        loop {
            let Ok(value) = self.listener.accept().await else {
                continue;
            };

            match self.stream_queue.len() < self.server_limits.max_pending_connections {
                true => self.stream_queue.push(value),
                false => self.error_queue.push(value),
            }
        }
    }

    #[inline]
    async fn get_stream(queue: &TcpQueue, wait: &WaitStrategy) -> (TcpStream, SocketAddr) {
        loop {
            if let Some(value) = queue.pop() {
                return value;
            }

            match wait {
                WaitStrategy::Yield => yield_now().await,
                WaitStrategy::Sleep(time) => tokio_sleep(*time).await,
            }
        }
    }
}

//

/// Builder for configuring and creating [`Server`] instances.
///
/// # Protocol Support
///
/// - `HTTP/1.X` (HTTP/1.1 or HTTP/1.1): Always enabled
/// - [`HTTP/0.9+`](crate::limits::Http09Limits): Optional,
///   enabled by setting [`http_09_limits`](Self::http_09_limits)
pub struct ServerBuilder<H, S = (), F = ()>
where
    H: Handler<S>,
    S: ConnectionData,
    F: ConnectionFilter,
{
    listener: Option<TcpListener>,
    handler: Option<Arc<H>>,
    connection_filter: Arc<F>,
    _marker: PhantomData<S>,

    server_limits: Option<ServerLimits>,
    request_limits: Option<ReqLimits>,
    response_limits: Option<RespLimits>,
    connection_limits: Option<ConnLimits>,
    http_09_limits: Option<Http09Limits>,
}

impl<H, S, F> ServerBuilder<H, S, F>
where
    H: Handler<S>,
    S: ConnectionData,
    F: ConnectionFilter,
{
    /// Sets the TCP listener that the server will use to accept connections.
    ///
    /// **This is a required component.**
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # maker_web::impt_default_handler!{ MyStruct }
    /// # #[tokio::main]
    /// # async fn main() {
    /// use tokio::net::TcpListener;
    /// use maker_web::Server;
    ///
    /// let server = Server::builder()
    ///     .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
    ///     .handler(MyStruct) // structure with Handler implementation
    ///     .build();
    /// # }
    /// ```
    #[inline(always)]
    pub fn listener(mut self, listener: TcpListener) -> Self {
        self.listener = Some(listener);
        self
    }

    /// Sets the request handler that will process incoming requests.
    ///
    /// **This is a required component.**
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use maker_web::{Server, Handler, Request, Response, Handled, StatusCode};
    /// use tokio::net::TcpListener;
    ///
    /// struct MyStruct;
    ///
    /// impl Handler for MyStruct {
    ///     async fn handle(&self, _: &mut (), _: &Request, resp: &mut Response) -> Handled {
    ///         resp.status(StatusCode::Ok).body("Hello World!")
    ///     }
    /// }
    ///
    /// # #[tokio::main]
    /// # async fn main() {
    /// let server = Server::builder()
    ///     .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
    ///     .handler(MyStruct)
    ///     .build();
    /// # }
    /// ```
    #[inline(always)]
    pub fn handler(mut self, handler: H) -> Self {
        self.handler = Some(Arc::new(handler));
        self
    }

    /// Installs a connection filter to check incoming TCP connections
    /// before using it.
    ///
    /// Allows early rejection of unwanted IP addresses (before the
    /// first read). Can be used for DDoS protection, geobanning, etc.
    ///
    /// For more information, see [ConnectionFilter](crate::ConnectionFilter)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # maker_web::impt_default_handler!{ MyStruct }
    /// use tokio::net::TcpListener;
    /// use std::net::SocketAddr;
    /// use maker_web::{ConnectionFilter, Server};
    ///
    /// struct MyConnFilter {
    ///     blacklist: Vec<SocketAddr>
    /// }
    ///
    /// impl ConnectionFilter for MyConnFilter {
    ///     fn filter(
    ///         &self, client_addr: SocketAddr, _: SocketAddr, err_resp: &mut Response
    ///     ) -> Result<(), Handled> {
    ///         if self.blacklist.contains(&client_addr) {
    ///             Err(err_resp
    ///                 .status(StatusCode::Forbidden)
    ///                 .body(b"Your IP is permanently banned"))
    ///         } else {
    ///             Ok(())
    ///         }
    ///     }
    /// }
    ///
    /// # #[tokio::main]
    /// # async fn main() {
    /// let filter = MyConnFilter {
    ///     blacklist: vec![
    ///         "192.0.2.1".parse().unwrap(),
    ///         "198.51.100.1".parse().unwrap(),
    ///         "203.0.113.1".parse().unwrap(),
    ///         "10.0.0.1".parse().unwrap(),
    ///     ]
    /// };
    ///
    /// let server = Server::builder()
    ///     .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
    ///     .handler(MyStruct) // structure with Handler implementation
    ///     .conn_filter(filter)
    ///     .build();
    /// # }
    /// ```
    #[inline(always)]
    pub fn conn_filter<NewF>(self, filter: NewF) -> ServerBuilder<H, S, NewF>
    where
        NewF: ConnectionFilter,
    {
        ServerBuilder {
            listener: self.listener,
            handler: self.handler,
            connection_filter: Arc::new(filter),
            _marker: self._marker,
            server_limits: self.server_limits,
            request_limits: self.request_limits,
            response_limits: self.response_limits,
            connection_limits: self.connection_limits,
            http_09_limits: self.http_09_limits,
        }
    }

    /// Configures request parsing and processing limits.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # maker_web::impt_default_handler!{ MyStruct }
    /// # #[tokio::main]
    /// # async fn main() {
    /// use maker_web::{Server, limits::ServerLimits};
    /// use tokio::net::TcpListener;
    /// use std::time::Duration;
    ///
    /// let server = Server::builder()
    ///     .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
    ///     .handler(MyStruct) // structure with Handler implementation
    ///     .server_limits(ServerLimits {
    ///         // Your changes
    ///         max_connections: 2500,
    ///         max_pending_connections: 10000,
    ///         ..ServerLimits::default() // Required line
    ///     })
    ///     .build();
    /// # }
    /// ```
    #[inline(always)]
    pub fn server_limits(mut self, limits: ServerLimits) -> Self {
        self.server_limits = Some(limits);
        self
    }

    /// Configures request parsing and processing limits.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # maker_web::impt_default_handler!{ MyStruct }
    /// # #[tokio::main]
    /// # async fn main() {
    /// use maker_web::{Server, limits::ConnLimits};
    /// use tokio::net::TcpListener;
    /// use std::time::Duration;
    ///
    /// let server = Server::builder()
    ///     .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
    ///     .handler(MyStruct) // structure with Handler implementation
    ///     .connection_limits(ConnLimits {
    ///         // Your changes
    ///         socket_read_timeout: Duration::from_secs(5),
    ///         socket_write_timeout: Duration::from_secs(2),
    ///         connection_lifetime: Duration::from_secs(200),
    ///         ..ConnLimits::default() // Required line
    ///     })
    ///     .build();
    /// # }
    /// ```
    #[inline(always)]
    pub fn connection_limits(mut self, limits: ConnLimits) -> Self {
        self.connection_limits = Some(limits);
        self
    }

    /// Enables and configures [`HTTP/0.9+`](crate::limits::Http09Limits) protocol support.
    ///
    /// # Note
    ///
    /// Omitting this call will completely disable HTTP/0.9+ support. The server
    /// will reject any HTTP/0.9+ requests, returning an error to the client.
    ///
    /// # Examples
    ///
    /// Enabling [`Http09Limits`]:
    /// ```no_run
    /// # maker_web::impt_default_handler!{ MyStruct }
    /// # #[tokio::main]
    /// # async fn main() {
    /// use maker_web::{Server, limits::Http09Limits};
    /// use tokio::net::TcpListener;
    ///
    /// let server = Server::builder()
    ///     .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
    ///     .handler(MyStruct) // structure with Handler implementation
    ///     .http_09_limits(Http09Limits::default())
    ///     .build();
    /// # }
    /// ```
    /// Change [`Http09Limits`]:
    /// ```no_run
    /// # maker_web::impt_default_handler!{ MyStruct }
    /// # #[tokio::main]
    /// # async fn main() {
    /// use maker_web::{Server, limits::Http09Limits};
    /// use tokio::net::TcpListener;
    ///
    /// let server = Server::builder()
    ///     .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
    ///     .handler(MyStruct) // structure with Handler implementation
    ///     .http_09_limits(Http09Limits {
    ///         // Your changes
    ///         max_requests_per_connection: 1000,
    ///         ..Http09Limits::default() // Required line
    ///     })
    ///     .build();
    /// # }
    /// ```
    #[inline(always)]
    pub fn http_09_limits(mut self, limits: Http09Limits) -> Self {
        self.http_09_limits = Some(limits);
        self
    }

    /// Configures request parsing and processing limits.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # maker_web::impt_default_handler!{ MyStruct }
    /// # #[tokio::main]
    /// # async fn main() {
    /// use maker_web::{Server, limits::ReqLimits};
    /// use tokio::net::TcpListener;
    ///
    /// let server = Server::builder()
    ///     .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
    ///     .handler(MyStruct) // structure with Handler implementation
    ///     .request_limits(ReqLimits {
    ///         // Your changes
    ///         url_size: 1024,
    ///         url_query_parts: 32,
    ///         url_parts: 20,
    ///         ..ReqLimits::default() // Required line
    ///     })
    ///     .build();
    /// # }
    /// ```
    #[inline(always)]
    pub fn request_limits(mut self, limits: ReqLimits) -> Self {
        self.request_limits = Some(limits);
        self
    }

    /// Configures response processing limits.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # maker_web::impt_default_handler!{ MyStruct }
    /// # #[tokio::main]
    /// # async fn main() {
    /// use maker_web::{Server, limits::RespLimits};
    /// use tokio::net::TcpListener;
    ///
    /// let server = Server::builder()
    ///     .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
    ///     .handler(MyStruct) // structure with Handler implementation
    ///     .response_limits(RespLimits {
    ///         // Your changes
    ///         default_capacity: 1024,
    ///         max_capacity: 4096,
    ///         ..RespLimits::default() // Required line
    ///     })
    ///     .build();
    /// # }
    /// ```
    #[inline(always)]
    pub fn response_limits(mut self, limits: RespLimits) -> Self {
        self.response_limits = Some(limits);
        self
    }

    /// Finalizes the builder and constructs a [`Server`] instance.
    ///
    /// # Panics
    ///
    /// Error messages:
    /// - ``The `listener` method must be called to create``
    /// - ``The `handler` method must be called to create``
    ///
    /// Panics when:
    /// - The `listener` method was not called.
    /// - The `handler` method was not called.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # maker_web::impt_default_handler!{ MyStruct }
    /// # #[tokio::main]
    /// # async fn main() {
    /// use tokio::net::TcpListener;
    /// use maker_web::Server;
    ///
    /// let server = Server::builder()
    ///     .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
    ///     .handler(MyStruct) // structure with Handler implementation
    ///     .build();
    ///
    /// // Yes, 3 identical examples, for you, in case you suddenly get lost :)
    /// #
    /// # // No, really. Documentation can be difficult for beginners.
    /// # }
    /// ```
    #[inline]
    #[track_caller]
    pub fn build(self) -> Server {
        let (listener, handler, filter, limits) = self.get_all_parts();

        let stream_queue = Arc::new(SegQueue::new());
        let error_queue = Arc::new(SegQueue::new());

        for _ in 0..limits.0.max_connections {
            Self::spawn_worker(&stream_queue, &limits, &filter, &handler);
        }
        if limits.0.count_503_handlers != 0 {
            for _ in 0..limits.0.count_503_handlers {
                Self::spawn_alarmist(&error_queue, &limits);
            }
        } else {
            Self::spawn_quiet_alarmist(&error_queue, &limits);
        }

        Server {
            listener,
            stream_queue,
            error_queue,
            server_limits: limits.0,
        }
    }

    #[inline]
    fn spawn_worker(queue: &TcpQueue, limits: &AllLimits, filter: &Arc<F>, handler: &Arc<H>) {
        let queue = queue.clone();
        let filter = filter.clone();
        let mut conn = HttpConnection::new(handler.clone(), limits.clone());

        tokio::spawn(async move {
            loop {
                let (mut stream, addr) =
                    Server::get_stream(&queue, &conn.server_limits.wait_strategy).await;

                let Ok(local_addr) = stream.local_addr() else {
                    continue;
                };

                if filter.filter(addr, local_addr, &mut conn.response).is_err()
                    || filter
                        .filter_async(addr, local_addr, &mut conn.response)
                        .await
                        .is_err()
                {
                    let _ = conn
                        .conn_limits
                        .write_bytes(&mut stream, conn.response.buffer())
                        .await;

                    conn.response.reset(&conn.resp_limits);
                    continue;
                }

                let _ = conn.run(&mut stream).await;
            }
        });
    }

    #[inline]
    fn spawn_alarmist(queue: &TcpQueue, limits: &AllLimits) {
        let queue = queue.clone();
        let (server_limits, conn_limits, ..) = limits.clone();

        tokio::spawn(async move {
            loop {
                let (mut stream, _) =
                    Server::get_stream(&queue, &server_limits.wait_strategy).await;

                let _ = conn_limits
                    .send_error(
                        &mut stream,
                        ErrorKind::ServiceUnavailable,
                        Version::Http11,
                        server_limits.json_errors,
                    )
                    .await;
            }
        });
    }

    #[inline]
    fn spawn_quiet_alarmist(queue: &TcpQueue, limits: &AllLimits) {
        let queue = queue.clone();
        let (server_limits, ..) = limits.clone();

        tokio::spawn(async move {
            loop {
                let (stream, _) = Server::get_stream(&queue, &server_limits.wait_strategy).await;

                drop(stream);
            }
        });
    }

    #[inline]
    #[track_caller]
    fn get_all_parts(self) -> (TcpListener, Arc<H>, Arc<F>, AllLimits) {
        (
            self.listener
                .expect("The `listener` method must be called to create"),
            self.handler
                .expect("The `handler` method must be called to create"),
            self.connection_filter,
            (
                self.server_limits.clone().unwrap_or_default(),
                self.connection_limits.clone().unwrap_or_default(),
                self.http_09_limits.clone(),
                self.request_limits
                    .clone()
                    .unwrap_or_default()
                    .precalculate(),
                self.response_limits.clone().unwrap_or_default(),
            ),
        )
    }
}

type TcpQueue = Arc<SegQueue<(TcpStream, SocketAddr)>>;
pub(crate) type AllLimits = (
    ServerLimits,
    ConnLimits,
    Option<Http09Limits>,
    ReqLimits,
    RespLimits,
);
