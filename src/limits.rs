//! Web server configuration limits and timeouts
//!
//! # Security-First Defaults
//!
//! Default limits are intentionally conservative to prevent:
//! - Resource exhaustion attacks
//! - Memory overflows  
//! - Slowloris attacks
//! - Header flooding
//!
//! # Memory Consumption
//!
//! Each active connection consumes memory according to:
//!
//! `Total` = [`Request Buffer`](crate::limits::ReqLimits#memory-allocation-strategy) +
//!           [`Response Buffer`](crate::limits::RespLimits#buffer-management) +
//!           `Runtime Overhead`
//!
//! See each component's documentation for details and configuration options.
//!
//! # Examples
//!
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

use std::time::Duration;

/// Controls server-level concurrency, queueing, and performance behavior.
///
/// Configures how the server handles connection admission, worker pools,
/// and overload protection with tunable parameters for different workloads.
///
/// # Connection management
/// ```text
///                            [------------]
///                            [ Tcp accept ]
///                            [------------]
///                                  ||
///                                  || TCP_STREAM
///                                  \/
/// [--------------]   Yes   /----------------\   No   [-------------]
/// [ Add to queue ] <====== | Queue if full? | =====> [ Sending 503 ]
/// [--------------]         \----------------/        [-------------]
///        ||                       
///        \==================\\          //====================\
///                            V          V                    ||
/// [---------]   Yes   /--------------------------\   No   [------]
/// [ Handler ] <====== | Is there a free handler? | =====> [ Wait ]
/// [---------]         \--------------------------/        [------]
/// ```
///
/// The queue acts as a buffer between connection acceptance and processing.
/// Workers continuously poll the queue using the configured `wait_strategy`.
///
/// # Handler
/// A worker process is a continuously running asynchronous task, created once
/// during initialization (from [tokio::spawn]). It runs in an infinite loop,
/// processing connections from a shared queue, which is replenished by a TCP
/// listener. This design eliminates the need to create tasks for each connection,
/// allowing for efficient resource reuse across an unlimited number of connections.
#[derive(Debug, Clone)]
pub struct ServerLimits {
    /// Maximum number of concurrent active connections being processed (default: `100`).
    ///
    /// When the server starts, exactly `max_connections` [handlers](#handler) are
    /// created and used.
    pub max_connections: usize,

    /// Maximum number of TCP connections waiting in the admission queue (default: `250`).
    ///
    /// All accepted connections first go into this queue. Worker processes select
    /// connections from here. If the queue becomes full, new connections receive immediate
    /// HTTP `503` responses.
    ///
    /// For more information, see [Connection management](#connection-management).
    pub max_pending_connections: usize,

    /// Strategy for worker task waiting behavior (default: `Sleep(50μs)`)
    ///
    /// Controls how worker tasks wait when connection buffers are empty
    /// (the size is set by field `max_pending_connections`). Affects latency,
    /// CPU usage, and throughput characteristics.
    pub wait_strategy: WaitStrategy,

    /// Dedicated handlers for queue overflow responses (default: `1`).
    ///
    /// When the connection queue becomes full, these handlers immediately send
    /// responses with the [503](crate::StatusCode::ServiceUnavailable) code. Using
    /// multiple handlers prevents bottlenecks in scenarios with a large volume of
    /// rejected requests. Set to 0 to silently close the connection (not recommended
    /// for production HTTP servers).
    pub count_503_handlers: usize,

    /// Format for error responses (default: `true`)
    ///
    /// # Examples
    /// If `true`, then on error the server will return:
    /// ```text
    /// HTTP/1.1 400 Bad Request\r
    /// connection: close\r
    /// content-length: 55\r
    /// content-type: application/json\r
    /// \r
    /// {"error":"Invalid HTTP method","code":"INVALID_METHOD"}
    /// ```
    /// If `false`, then on error the server will return:
    /// ```text
    /// HTTP/1.1 400 Bad Request\r
    /// connection: close\r
    /// content-length: 0\r
    /// \r
    /// ```
    pub json_errors: bool,

    #[doc(hidden)]
    #[allow(dead_code)]
    pub _priv: (),
}

impl Default for ServerLimits {
    fn default() -> Self {
        Self {
            max_connections: 100,
            max_pending_connections: 250,
            wait_strategy: WaitStrategy::Sleep(Duration::from_micros(50)),
            count_503_handlers: 1,
            json_errors: true,

            _priv: (),
        }
    }
}

/// Strategy for worker task waiting when no connections are available
///
/// Different strategies optimize for different workload patterns.
/// Choose based on your latency requirements and resource constraints.
#[derive(Debug, Clone)]
pub enum WaitStrategy {
    /// While waiting, uses [`tokio::task::yield_now()`]
    ///
    /// # Note
    /// According to personal measurements, when using this option, the CPU load
    /// is 97-99%, so I do not recommend using it.
    ///
    /// Server operation with this waiting strategy:
    /// ```
    /// # #[tokio::main]
    /// async fn main() {
    /// # let mut pool = vec![1, 2, 3];
    /// #
    /// let value = loop {
    ///     if let Some(value) = pool.pop() {
    ///         break value;
    ///     }
    ///
    ///     tokio::task::yield_now().await;
    /// };
    /// # }
    /// ```
    Yield,

    /// While waiting, uses [`tokio::time::sleep()`]
    ///
    /// Server operation with this waiting strategy:
    /// ```
    /// # #[tokio::main]
    /// async fn main() {
    /// # let mut pool = vec![1, 2, 3];
    /// # let time = std::time::Duration::from_micros(50);
    /// #
    /// let value = loop {
    ///     if let Some(value) = pool.pop() {
    ///         break value;
    ///     }
    ///
    ///     tokio::time::sleep(time).await;
    /// };
    /// # }
    /// ```
    Sleep(Duration),
}

/// Connection-level limits and timeouts
///
/// Controls individual TCP connection behavior including timeouts,
/// lifetime, and request limits.
///
/// Default values balance performance, resource usage, and security.
/// Only change if you understand the consequences.
#[derive(Debug, Clone)]
pub struct ConnLimits {
    /// Maximum duration to wait for reading data from socket (default: `2 seconds`)
    ///
    /// If no data is received within this time, connection is closed.
    /// This is the primary mechanism for cleaning up stalled connections.
    /// Prevents `slowloris attacks` and frees resources from inactive clients.
    pub socket_read_timeout: Duration,

    /// Maximum duration to wait for writing data to socket (default: `3 seconds`)
    ///
    /// If data can't be written in time, connection is terminated.
    /// Applies to individual write operations.
    pub socket_write_timeout: Duration,

    /// Maximum number of requests allowed per connection (default: `100`)
    ///
    /// Connection closes after processing this many requests.
    /// Helps prevent potential memory accumulation and maintains connection health.
    /// Combined with `connection_lifetime`, ensures connections don't live indefinitely.
    pub max_requests_per_connection: usize,

    /// Maximum lifetime of connection from establishment to closure (default: `2 minutes`)
    ///
    /// Final safety net that guarantees no connection lives longer than this duration.
    /// In practice, connections are typically cleaned up by `socket_read_timeout`
    /// or `max_requests_per_connection` long before this limit is reached.
    ///
    /// This also protects against business logic that takes very long time to execute
    /// (e.g., query parsing: 0.05s + business logic: 5s = connection could last 16 minutes
    /// excluding I/O operations without this limit).
    pub connection_lifetime: Duration,

    #[doc(hidden)]
    #[allow(dead_code)]
    pub _priv: (),
}

impl Default for ConnLimits {
    #[inline(always)]
    fn default() -> Self {
        Self {
            socket_read_timeout: Duration::from_secs(2),
            socket_write_timeout: Duration::from_secs(3),
            connection_lifetime: Duration::from_secs(120),
            max_requests_per_connection: 100,

            _priv: (),
        }
    }
}

/// Configuration for `HTTP/0.9+` protocol support
///
/// HTTP/0.9+ is an optimized protocol variant for high-performance scenarios
/// that maintains backward compatibility with original HTTP/0.9 while adding
/// modern features like keep_alive connections and query string support.
///
/// # Protocol Features
///
/// - **Ultra-minimal format**:
///   [See transcript](crate::Request#general-designations)
///   ```text
///   [METHOD] SP [PATH] CRLF
///   ```
/// - **Keep_alive support**: Paths starting with `/keep_alive/` maintain persistent connections
/// - **Query strings**: Full URL parsing with query parameters supported
/// - **Zero overhead**: No headers, status codes, or other `HTTP/1.x` metadata
/// - **Support all HTTP methods**:
///   
///   Starting with `maker_web@0.1.2` `HTTP/0.9+` starts supporting
///   [all methods](crate::Method) that `HTTP/1.x`.
///  
///   **Examples**:
///   ```text
///   GET /users/123/posts\r\n
///   POST /orders\r\n
///   PUT /products/456\r\n
///   DELETE /comments/789\r\n
///   HEAD /api/status\r\n
///   PATCH /profile/settings\r\n
///   OPTIONS /auth\r\n
///   GET /feed?page=2&limit=20\r\n
///   DELETE /sessions?all=true\r\n
///   ```
///   And it all works now :D
///
/// # Request Format
///
/// ```text
/// Standard:     GET /path\r\n
/// Keep_alive:   POST /keep_alive/path\r\n  
/// With query:   PUT /path?param=value\r\n
/// Combined:     HEAD /keep_alive/path?param=value\r\n
/// ```
///
/// # Response Format  
///
/// ```text
/// Raw response body without headers
/// Connection closes unless `keep_alive` path used
/// ```
///
/// # Error Handling
///
/// - **Client errors**: `ERROR: [code] [message]\r\n` response for malformed
///   requests (e.g., `ERROR: 400 Bad Request\r\n`, `ERROR: 404 Not Found\r\n`)
/// - **Server failures**: Immediate connection termination for I/O errors and
///   timeout conditions
///
/// # Keep-Alive Management
///
/// Connections are automatically closed when:
/// - **Timeout reached**: no requests within the
///   [`set time limit`](ConnLimits::socket_read_timeout)
/// - **Request limit exceeded**: Processed
///   [`max_requests_per_connection`](Http09Limits::max_requests_per_connection)
///   on single connection  
/// - **Explicit close**: Use non-keep_alive path to close connection
///
/// To explicitly close a keep_alive connection:
/// ```text
/// GET /close-connection\r\n  # Regular path (no /keep_alive/) closes connection
/// ```
///
/// # Protocol Compatibility & Usage Strategies
///
/// ## Pure HTTP/0.9+ (Maximum Performance)
///
/// Ideal for controlled environments where you implement both client and server:
/// ```no_run
/// // Simple HTTP/0.9+ client in 16 lines:
/// use std::{net::TcpStream, io::{Result, Read, Write}};
///
/// # fn main() -> Result<()> {
/// let mut stream = TcpStream::connect("localhost:8080")?;
/// let response = http09_client(&mut stream, "GET /hello/world\r\n")?;
/// println!("{}", response);
/// # Ok(())
/// # }
///
/// fn http09_client(stream: &mut TcpStream, request: &str) -> Result<String> {
///     // Sending a request
///     stream.write_all(request.as_bytes())?;
///
///     // Reading a response
///     let mut line = String::new();
///     stream.read_to_string(&mut line)?;
///     
///     Ok(line)
/// }
/// ```
///
/// ## Hybrid HTTP/1.X + HTTP/0.9+ (Browser & Complex Scenarios)
///
/// **Note**: HTTP/1.X requests interleaved in a connection are subject to [`ConnLimits`].
///
/// Combine protocols when you need advanced features or browser compatibility:
///
/// - **Initial setup with HTTP/1.X** (authentication, complex headers):
///    ```text
///    GET /api HTTP/1.1\r
///    Host: localhost\r
///    Authorization: Basic YWxhZGRpbjpvcGVuc2VzYW1l\r
///    User-Agent: curl/7.64.1\r
///    \r\n
///    ```
///
/// - **High-frequency data with HTTP/0.9+**:
///    ```text
///    GET /keep_alive/api/user/first_name\r
///    GET /keep_alive/api/user/last_name\r
///    GET /keep_alive/api/user/country\r
///    GET /keep_alive/api/user/city\r\n
///
///    POST /keep_alive/api/user?last_name=Qwe\r\n
///    POST /keep_alive/api/user?first_name=Rty\r\n
///    POST /keep_alive/api/user?city=123\r\n
///
///    DELETE /keep_alive/api/user/123\r\n
///    ```
///
/// - **Final request** (is selected depending on the conditions):
///    - HTTP/0.9+:
///      ```text
///      GET /api/user/time\r\n`
///      ```
///    - HTTP/1.X:
///      ```text  
///      POST /api/user/time HTTP/1.1\r
///      Host: localhost\r
///      Connection: close\r
///      \r\n
///      ```
///
/// ## HTTP/1.X Only (Public APIs & Maximum Compatibility)
///
/// Use standard HTTP/1.X when:
/// - Serving public APIs to unknown clients
/// - Browser compatibility is required without custom client implementation
/// - Advanced HTTP features are needed (CORS, caching headers, etc.)
///
/// # Limits
///
/// This setting only works for `HTTP/0.9+`. The same-named fields in [`ConnLimits`]
/// are ignored for this protocol.
#[derive(Debug, Clone)]
pub struct Http09Limits {
    /// Maximum number of requests per keep_alive connection (default: `250`)
    ///
    /// Connection automatically closes after processing this many requests,
    /// even if the timeout hasn't been reached. This prevents potential
    /// memory leaks and resource exhaustion in long-running connections.
    ///
    /// # Examples
    /// - Value of `250`: connection handles up to 250 requests then closes
    /// - Value of `1`: effectively disables keep_alive (closes after each request)
    /// - Value of `usize::MAX`: no limit (use with caution)
    pub max_requests_per_connection: usize,

    /// Keep_alive connection timeout (default: `30 seconds`)  
    ///
    /// Maximum idle time between requests before closing persistent connections.
    /// Timer resets on each new request. Shorter timeouts free resources faster
    /// but may increase TCP connection overhead due to more frequent handshakes.
    ///
    /// # Trade-offs
    /// - Shorter (5-10s): better resource cleanup, higher connection overhead
    /// - Longer (30-60s): lower overhead, but resources held longer
    /// - Very long (5+ minutes): not recommended outside controlled environments
    pub connection_lifetime: Duration,

    #[doc(hidden)]
    #[allow(dead_code)]
    pub _priv: (),
}

impl Default for Http09Limits {
    fn default() -> Self {
        Self {
            max_requests_per_connection: 250,
            connection_lifetime: Duration::from_secs(30),
            _priv: (),
        }
    }
}

/// HTTP request parsing limits and buffer pre-allocation strategy
///
/// ⚠️ **SECURITY-FIRST DEFAULTS**
///
/// These limits are intentionally conservative to prevent resource exhaustion
/// and various parsing attacks. They work well for:
/// - Simple REST APIs
/// - Microservices
/// - Internal tools
/// - Low-memory environments
///
/// 🔧 **You MAY need to increase these if you see:**
/// - `413 Payload Too Large` for legitimate requests  
/// - `414 URI Too Long` for normal API calls
/// - `431 Request Header Fields Too Large`
///
/// # Memory Allocation Strategy
///
/// Each TCP connection pre-allocates a fixed-size buffer based on these limits:
///
/// ```text
/// Total Buffer = First Line + (Headers × Header Line) + Body + Overhead
/// ```
///
/// ## Buffer Size Calculation (Default Values)
///
/// | Component | Formula | Size | Purpose |
/// |-----------|---------|------|---------|
/// | First Line | `19 + url_size` | 275 B | `METHOD URL HTTP/1.1\r\n` |
/// | Headers | `header_count × Header Line` | 9,280 B | Headers storage |
/// | Header Line | `header_name_size + header_value_size + 4` | 580 B | `Name: Value\r\n` |
/// | Body | `body_size` | 4,096 B | Request payload |
/// | **Total** | **Sum + 2 bytes CRLF + struct (64 B)** | **13,717 B = ~13.4 KB** | Per connection buffer |
///
/// # Memory Planning
///
/// # Example
/// ```
/// use maker_web::limits::ReqLimits;
///
/// let mut limits = ReqLimits::default();
/// let buffer_size = limits.estimated_buffer_size();
/// println!("Each connection needs {} bytes for data buffer", buffer_size);
/// ```
///
/// # Trade-off Considerations
///
/// - **Small limits**: Less memory, faster parsing, but may reject legitimate requests
/// - **Large limits**: More memory overhead, but handles complex APIs and large payloads
///
/// Adjust based on your specific use case and available resources.
#[derive(Debug, Clone)]
pub struct ReqLimits {
    /// Maximum URL length in bytes including path and query string (default: `256 B`)
    ///
    /// Covers the entire URL after the method (e.g., `/api/users/123?sort=name&debug`).
    /// Most REST APIs fit within 256 bytes. Increase if you have long query parameters
    /// or deeply nested paths.
    pub url_size: usize,
    /// Maximum number of path segments in URL (default: `8 segments`)
    ///
    /// Counts slashes in path (e.g., `/api/users/123` has 3 segments).
    /// Sufficient for most REST APIs. Increase for very deep nesting.
    pub url_parts: usize,
    /// Maximum query string length (default: `128`)
    ///
    /// Covers the entire query request, including `?` (e.g., `?sort=name&debug`).
    /// If you don't need this limit, set it to [url_size](Self::url_size).
    pub url_query_size: usize,
    /// Maximum number of query parameters (default: `8`)
    ///
    /// Limits the URL query string to N `key=value` pairs separated by `&` when N > 1
    /// (e.g., `?sort=name&debug` has 2 pairs, `?sort=name&debug=true&page=1` has 3 pairs).
    /// Prevents query parameter explosion attacks.
    /// Increase for complex filtering APIs with many parameters.
    pub url_query_parts: usize,

    /// Maximum number of headers per request (default: `16 headers`)
    ///
    /// Typical browsers send 10-12 headers. 16 provides room for custom headers
    /// while preventing header flooding attacks.
    pub header_count: usize,
    /// Maximum header name length in bytes (default: `64 B`)
    ///
    /// Standard header names are short (`content-type`, `authorization`).
    /// 64 bytes accommodates custom headers like `x-custom-header-name`.
    pub header_name_size: usize,
    /// Maximum header value length in bytes (default: `512 B`)
    ///
    /// Fits most headers including JWT tokens, cookies, and UUIDs.
    /// Increase for large cookies or complex authentication tokens.
    pub header_value_size: usize,

    /// Maximum request body size in bytes (default: `4 KB`)
    ///
    /// Suitable for API requests with JSON payloads. Increase for file uploads
    /// or large data submissions. Set based on your expected payload sizes.
    pub body_size: usize,

    #[doc(hidden)]
    #[allow(dead_code)]
    pub precalc: ReqLimitsPrecalc,
}

impl Default for ReqLimits {
    fn default() -> Self {
        Self {
            // Security-conscious defaults
            url_size: 256,       // Enough for: /api/v1/users/search?q=test&page=1
            url_parts: 8,        // /api/users/123
            url_query_size: 128, // Enough for: ?sort=name&debug
            url_query_parts: 8,  // ?sort=name&debug

            header_count: 16,       // Typical: 10-12 browser headers + 4-6 custom
            header_name_size: 64,   // Fits: x-custom-auth-token-header-name
            header_value_size: 512, // Fits most JWT tokens and cookies

            body_size: 4 * 1024, // Good for JSON API requests, not file uploads

            precalc: ReqLimitsPrecalc::default(),
        }
    }
}

impl ReqLimits {
    /// Returns the estimated memory buffer size required per connection.
    /// Identical to [std::mem::size_of_val]
    ///
    /// This calculates the total buffer size needed to parse HTTP requests
    /// based on the current limits. The buffer includes space for:
    /// - HTTP request line
    /// - Headers (name + value for each header)  
    /// - Request body
    /// - CRLF terminators
    ///
    /// # Note
    /// The returned size represents only the data buffer. Additional memory
    /// is used for the parser structure itself (~64 bytes).
    ///
    /// # Example
    /// ```
    /// use maker_web::limits::ReqLimits;
    ///
    /// let mut limits = ReqLimits::default();
    /// let buffer_size = limits.estimated_buffer_size();
    /// println!("Each connection needs {} bytes for data buffer", buffer_size);
    /// ```
    #[inline(always)]
    pub fn estimated_buffer_size(self) -> usize {
        self.precalculate().precalc.buffer
    }

    #[inline(always)]
    pub(crate) fn precalculate(mut self) -> Self {
        self.precalc.first_line = self.first_line();
        self.precalc.h_line = self.h_line();
        self.precalc.buffer = self.buffer();
        self.precalc.req_without_body = self.precalc.buffer - self.body_size;

        self
    }

    #[inline(always)]
    // First line + Header * N + "\r\n" + Body
    fn buffer(&self) -> usize {
        self.precalc.first_line + self.header_count * self.precalc.h_line + 2 + self.body_size
    }

    #[inline(always)]
    // First line HTTP response:
    // CONNECT /url/test HTTP/1.1\r\n
    // |-----| |-------| |------|
    //  Method    URl    Version
    //
    // Formula: Method(7) + " " + URl + " " + Version(8) + "\r\n"
    // In Code: 19 + url_size
    fn first_line(&self) -> usize {
        19 + self.url_size
    }

    #[inline(always)]
    // Header:
    // Authorization: Sample%20Data\r\n
    // |-----------|  |-----------|
    //     Name           Value
    //
    // Formula: Name + ": " + Value +  "\r\n"
    // In Code: 4 + header_name_size + header_value_size
    fn h_line(&self) -> usize {
        self.header_name_size + self.header_value_size + 4
    }
}

#[doc(hidden)]
#[derive(Debug, Clone, Default)]
pub struct ReqLimitsPrecalc {
    pub(crate) buffer: usize,
    pub(crate) first_line: usize,
    pub(crate) req_without_body: usize,
    pub(crate) h_line: usize,
}

/// Configuration for response processing and memory allocation limits.
///
/// Controls how response buffers are allocated and managed to balance
/// memory usage and performance.
///
/// # Buffer Management
///
/// Based on the configured limits, response buffers are managed as follows:
/// ```rust
/// # use maker_web::limits::RespLimits;
/// # let limits = RespLimits::default();
/// # let mut buffer: Vec<()> = Vec::with_capacity(limits.default_capacity);
/// #
/// // `buffer` is Vec
/// if buffer.capacity() > limits.max_capacity {
///     buffer = Vec::with_capacity(limits.default_capacity);
/// } else {
///     buffer.clear();
/// }
/// ```
///
/// When the server starts, buffers are created with a capacity equal to `default_capacity`.
#[derive(Debug, Clone)]
pub struct RespLimits {
    /// Initial buffer capacity allocated for responses (default: `1024 B`)
    pub default_capacity: usize,
    /// Maximum allowed buffer capacity for responses (default: `8192 B`)
    //
    // Note: If the response exceeds `max_capacity * 2`, it may be sent in 1 or more `syscall`
    pub max_capacity: usize,

    #[doc(hidden)]
    #[allow(dead_code)]
    pub _priv: (),
}

impl Default for RespLimits {
    fn default() -> Self {
        Self {
            default_capacity: 1024,
            max_capacity: 8 * 1024,

            _priv: (),
        }
    }
}
