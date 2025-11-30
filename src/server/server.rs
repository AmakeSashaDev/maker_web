use crate::{
    errors::ErrorKind,
    http::{
        request::Request,
        response::{Handled, Response},
    },
    limits::{ConnLimits, Http09Limits, ReqLimits, RespLimits, ServerLimits, WaitStrategy},
    server::connection::{writer, ConnectionData, HttpConnection},
    Version,
};
use crossbeam::queue::{ArrayQueue, SegQueue};
use std::{
    future::Future,
    marker::{PhantomData, Send, Sync},
    sync::{atomic::Ordering, Arc},
};
use tokio::{
    net::{TcpListener, TcpStream},
    task::yield_now,
    time::sleep as tokio_sleep,
};

/// A trait for handling HTTP requests and generating responses.
///
/// You can use:
/// - `&self` to store shared data (e.g. database, router)
/// - `&mut S` to store connection data (e.g. authentication tokens, user settings)
///
/// # Examples
///
/// Basic Request Handler
/// ```
/// use maker_web::{Handler, Request, Response, Handled, StatusCode};
///
/// struct MyHandler;
///
/// impl Handler<()> for MyHandler {
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
///             .body(format!("Request count: {}", data.request_count))
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
pub trait Handler<S>
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
        req: &Request,
        resp: &mut Response,
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
/// impl Handler<()> for MyHandler {
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
pub struct Server<H: Handler<S>, S: ConnectionData> {
    listener: TcpListener,
    _marker: PhantomData<S>,

    worker_pool: Arc<ArrayQueue<HttpConnection<H, S>>>,
    incoming_streams: Arc<SegQueue<TcpStream>>,

    server_limits: ServerLimits,
}

macro_rules! impl_get_value_queue {
    ($name:ident, $type:ident) => {
        #[inline]
        async fn $name<V>(pool: &Arc<$type<V>>, limits: &ServerLimits) -> V {
            loop {
                if let Some(value) = pool.pop() {
                    return value;
                }

                match &limits.wait_strategy {
                    WaitStrategy::Yield => yield_now().await,
                    WaitStrategy::Sleep(time) => tokio_sleep(*time).await,
                }
            }
        }
    };
}

impl<H: Handler<S>, S: ConnectionData> Server<H, S> {
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
    #[inline(always)]
    pub fn builder() -> ServerBuilder<H, S> {
        ServerBuilder {
            listener: None,
            handler: None,
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
        let streams_rx = self.incoming_streams.clone();
        let worker_pool_0 = self.worker_pool.clone();
        let server_limits_0 = self.server_limits.clone();

        tokio::spawn(async move {
            let streams_tx = self.incoming_streams.clone();

            loop {
                let Ok((stream, _)) = self.listener.accept().await else {
                    continue;
                };

                Self::push_incoming_streams(stream, &streams_tx, &server_limits_0);
            }
        });

        loop {
            let mut worker = Self::get_value_array(&worker_pool_0, &self.server_limits).await;
            let mut stream = Self::get_value_seq(&streams_rx, &self.server_limits).await;

            let worker_pool = self.worker_pool.clone();
            tokio::spawn(async move {
                let _ = worker.run(&mut stream).await;
                let _ = worker_pool.push(worker);
            });
        }
    }

    #[inline]
    fn push_incoming_streams(
        mut stream: TcpStream,
        streams_tx: &Arc<SegQueue<TcpStream>>,
        server_limits: &ServerLimits,
    ) {
        if streams_tx.len() < server_limits.max_pending_connections {
            streams_tx.push(stream);
        } else {
            tokio::spawn(async move {
                let _ =
                    writer::send_error(&mut stream, Version::Http11, ErrorKind::ServiceUnavailable)
                        .await;
            });
        }
    }

    impl_get_value_queue! { get_value_array, ArrayQueue }
    impl_get_value_queue! { get_value_seq, SegQueue }
}

//

/// Builder for configuring and creating [`Server`] instances.
///
/// # Protocol Support
///
/// - `HTTP/1.X` (HTTP/1.1 or HTTP/1.1): Always enabled
/// - [`HTTP/0.9+`](crate::limits::Http09Limits): Optional,
///   enabled by setting [`http_09_limits`](Self::http_09_limits)
pub struct ServerBuilder<H: Handler<S>, S: ConnectionData> {
    listener: Option<TcpListener>,
    handler: Option<H>,
    _marker: PhantomData<S>,

    server_limits: Option<ServerLimits>,
    request_limits: Option<ReqLimits>,
    response_limits: Option<RespLimits>,
    connection_limits: Option<ConnLimits>,
    http_09_limits: Option<Http09Limits>,
}

impl<H: Handler<S>, S: ConnectionData> ServerBuilder<H, S> {
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

    /// Sets the request handler will process incoming requests.
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
    pub fn handler(mut self, handler: H) -> Self {
        self.handler = Some(handler);
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
    /// Switching on [`Http09Limits`]:
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
    ///# }
    /// ```
    #[inline(always)]
    #[track_caller]
    pub fn build(self) -> Server<H, S> {
        let (listener, handler, limits) = self.get_all_limits();
        Self::store_atomic(&limits);

        let handler = Arc::new(handler);
        let worker_pool = ArrayQueue::new(limits.0.max_connections);

        for _ in 0..limits.0.max_connections {
            let value = HttpConnection::new(handler.clone(), limits.clone());
            let _ = worker_pool.push(value);
        }

        Server {
            listener,
            _marker: PhantomData,

            worker_pool: Arc::new(worker_pool),
            incoming_streams: Arc::new(SegQueue::new()),

            server_limits: limits.0,
        }
    }

    #[inline(always)]
    fn store_atomic(limits: &AllTypesLimits) {
        writer::SOCKET_WRITE_TIMEOUT.store(
            limits
                .1
                .socket_write_timeout
                .as_micros()
                .try_into()
                .unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );

        writer::JSON_ERRORS.store(limits.0.json_errors, Ordering::Relaxed);
    }

    #[inline(always)]
    #[track_caller]
    fn get_all_limits(self) -> (TcpListener, H, AllTypesLimits) {
        (
            self.listener
                .expect("The `listener` method must be called to create"),
            self.handler
                .expect("The `handler` method must be called to create"),
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

pub(crate) type AllTypesLimits = (
    ServerLimits,
    ConnLimits,
    Option<Http09Limits>,
    ReqLimits,
    RespLimits,
);
