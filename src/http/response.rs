//! A highly efficient, zero-allocation HTTP response builder for embedded web servers.

use crate::{
    http::types::{StatusCode, Version},
    limits::RespLimits,
    BodyWriter, WriteBuffer,
};
use std::{borrow::Cow, rc::Rc, sync::Arc};

#[derive(Debug)]
/// HTTP response builder for constructing server responses.
///
/// Provides a fluent interface for building HTTP responses with status codes,
/// headers, and body content. Automatically handles content-length calculation
/// and connection management.
///
/// Build responses by chaining methods in strict order:
/// - `HTTP/1.x`: [`status()`](Response::status) -> headers ->
///   any body method
/// - `HTTP/0.9`: Any `HTTP/0.9+` method
///
/// **To disable [`HTTP/0.9+`](crate::limits::Http09Limits) support, omit
/// [`http_09_limits`](crate::ServerBuilder::http_09_limits)
/// when creating the [`Server`](crate::Server).**
///
/// Instances are created automatically by the server and passed to
/// the [`Handler::handle`](crate::Handler::handle).
///
/// # Examples
/// ```
/// use maker_web::{Handled, Request, Response, StatusCode};
///
/// // In your implementation `Handler`
/// async fn handle(_req: &Request, resp: &mut Response) -> Handled {
///     resp
///         .status(StatusCode::Ok)
///         .header("content-type", "text/html")
///         .body("<h1>Hello World</h1>")
/// }
/// ```
///
/// # Panics
/// All methods perform validity checks in `debug` mode that panic on violations.
/// In `release` mode, these checks are omitted for performance, which may
/// produce invalid HTTP responses. Before creating a release version, conduct tests.
pub struct Response {
    buffer: Vec<u8>,
    pub(crate) version: Version,
    pub(crate) keep_alive: bool,
    posit_length: usize,
    start_body: usize,
    state: ResponseState,
}

#[doc(hidden)]
pub struct Handled(());

#[derive(Debug, Clone, Copy, PartialEq)]
enum ResponseState {
    Clean,
    Headers,
    Complete,
}

impl Response {
    #[inline(always)]
    pub(crate) fn new(limits: &RespLimits) -> Self {
        Self {
            buffer: Vec::with_capacity(limits.default_capacity),
            version: Version::Http11,
            keep_alive: true,
            posit_length: 0,
            start_body: 0,
            state: ResponseState::Clean,
        }
    }

    #[inline(always)]
    pub(crate) fn reset(&mut self, limits: &RespLimits) {
        if self.buffer.capacity() > limits.max_capacity {
            self.buffer = Vec::with_capacity(limits.default_capacity);
        } else {
            self.buffer.clear();
        }

        self.version = Version::Http11;
        self.keep_alive = true;
        self.posit_length = 0;
        self.start_body = 0;
        self.state = ResponseState::Clean;
    }

    #[inline(always)]
    pub(crate) fn buffer(&self) -> &Vec<u8> {
        &self.buffer
    }
}

/// Methods that work with all protocols
impl Response {
    /// Forces the connection to close after a response.
    ///
    /// # Examples
    /// ```
    /// # maker_web::run_test(|req, resp| {
    /// use maker_web::{StatusCode, Version};
    ///
    /// if req.version() == Version::Http09 {
    ///     resp.close().http09("Closing connection")
    /// } else {
    ///     resp.status(StatusCode::Ok)
    ///         .close()  // Connection will close after this response
    ///         .body("Closing connection")
    /// }
    /// # });
    /// ```
    ///
    /// # Panics
    /// Error messages:
    /// - `Must be called before any finalizing method`
    ///
    /// Panics in `debug` mode when:
    /// - Called after any finalizing method (method returning `Handler`)
    #[inline]
    #[track_caller]
    pub fn close(&mut self) -> &mut Self {
        debug_assert!(
            self.state != ResponseState::Complete,
            "Must be called before any finalizing method",
        );

        self.keep_alive = false;
        self
    }
}

/// Methods for working with `HTTP/1.X` (HTTP/1.1 or HTTP/1.1)
impl Response {
    /// Sets the HTTP status code for the response.
    ///
    /// # Examples
    /// ```
    /// # maker_web::run_test(|_, resp| {
    /// use maker_web::StatusCode;
    ///
    /// resp.status(StatusCode::NotFound)
    ///     .body(r#"{"status": "not found", "code": 404}"#)
    /// # });
    /// ```
    ///
    /// # Panics
    /// Error messages:
    /// - `Must be first and called only once`
    /// - <code>This method is only for \`HTTP/1.X\`</code>
    ///
    /// Panics in `debug` mode when:
    /// - Called multiple times
    /// - Called after any body method
    /// - Called for a non-HTTP/1.X response
    #[inline]
    #[track_caller]
    pub fn status(&mut self, status: StatusCode) -> &mut Self {
        debug_assert!(
            self.state == ResponseState::Clean,
            "Must be first and called only once"
        );
        debug_assert!(
            self.version != Version::Http09,
            "This method is only for `HTTP/1.X`"
        );

        self.buffer
            .extend_from_slice(status.to_first_line(self.version));
        self.state = ResponseState::Headers;
        self
    }

    /// Adds a header to the response.
    ///
    /// PLEASE DO NOT ADD THE FOLLOWING HEADINGS:
    /// - `content-length` - calculated automatically
    /// - `connection` - use [`close()`](Response::close)
    ///
    /// # Examples
    /// ```
    /// # maker_web::run_test(|_, resp| {
    /// use maker_web::StatusCode;
    ///
    /// resp.status(StatusCode::Ok)
    ///     .header("content-type", "text/plain")        // &str, &str
    ///     .header("x-custom-id", 128)                  // &str, i32  
    ///     .header("x-cache-enabled", true)             // &str, bool
    ///     .body("Done")
    /// # });
    /// ```
    ///
    /// # Panics
    /// Error message: `Must be called after status() and before any body method`
    ///
    /// Panics in `debug` mode when:
    /// - Called before [`status()`](Response::status)
    /// - Called after [`body()`](Response::body) or [`body_with()`](Response::body_with)
    #[inline]
    #[track_caller]
    pub fn header<N: WriteBuffer, V: WriteBuffer>(&mut self, name: N, value: V) -> &mut Self {
        debug_assert!(
            self.state == ResponseState::Headers,
            "Must be called after status() and before any body method"
        );

        name.write_to(&mut self.buffer);
        self.buffer.extend_from_slice(b": ");
        value.write_to(&mut self.buffer);
        self.buffer.extend_from_slice(b"\r\n");
        self
    }

    /// Add a multi-value header to the response
    ///
    /// PLEASE DO NOT ADD THE FOLLOWING HEADINGS:
    /// - `content-length` - calculated automatically
    /// - `connection` - use [`close()`](Response::close)
    ///
    /// # Examples
    /// ```
    /// # maker_web::run_test(|_, resp| {
    /// use maker_web::StatusCode;
    ///
    /// resp.status(StatusCode::Ok)
    ///     .header_multi(b"x-tags", ", ", ["user"])
    ///     // x-tags: user
    ///     .header_multi("accept", "; ", ["text/html", "text/plain"])
    ///     // accept: text/html; text/plain
    ///     .header_multi("id-users", ", ", vec![123, 234, 345])
    ///     // id-users: 123, 234, 345
    ///     .body("Done")
    /// # });
    /// ```
    ///
    /// # Panics
    /// Error message: `Must be called after status() and before any body method`
    ///
    /// Panics in `debug` mode when:
    /// - Called before [`status()`](Response::status)
    /// - Called after [`body()`](Response::body) or [`body_with()`](Response::body_with)
    #[inline]
    #[track_caller]
    pub fn header_multi<N, S, I, V>(&mut self, name: N, split: S, values: I) -> &mut Self
    where
        N: WriteBuffer,
        S: WriteBuffer,
        I: IntoIterator<Item = V>,
        V: WriteBuffer,
    {
        debug_assert!(
            self.state == ResponseState::Headers,
            "Must be called after status() and before any body method"
        );

        name.write_to(&mut self.buffer);
        self.buffer.extend_from_slice(b": ");

        let mut iter = values.into_iter();
        if let Some(first) = iter.next() {
            first.write_to(&mut self.buffer);

            for value in iter {
                split.write_to(&mut self.buffer);
                value.write_to(&mut self.buffer);
            }
        }

        self.buffer.extend_from_slice(b"\r\n");
        self
    }

    /// Adds a header with parameters to the response.
    ///
    /// PLEASE DO NOT ADD THE FOLLOWING HEADINGS:
    /// - `content-length` - calculated automatically
    /// - `connection` - use [`close()`](Response::close)
    ///
    /// # Examples
    /// ```
    /// # maker_web::run_test(|_, resp| {
    /// use maker_web::StatusCode;
    ///
    /// resp.status(StatusCode::Ok)
    ///     .header_params("content-type", "; ", vec![
    ///         ("text/html", None),
    ///         ("charset", Some("utf-8")),
    ///     ])
    ///     // Content-Type: text/html; charset=utf-8
    ///     .header_params("cache-control", ", ", [
    ///         ("max-age", Some("3600")),
    ///         ("must-revalidate", None),
    ///     ])
    ///     // Cache-Control: max-age=3600, must-revalidate
    ///     .body("Done")
    /// # });
    /// ```
    ///
    /// # Panics
    /// Error message: `Must be called after status() and before any body method`
    ///
    /// Panics in `debug` mode when:
    /// - Called before [`status()`](Response::status)
    /// - Called after [`body()`](Response::body) or [`body_with()`](Response::body_with)
    #[inline]
    #[track_caller]
    pub fn header_params<N, S, I, K, V>(&mut self, name: N, split: S, params: I) -> &mut Self
    where
        N: WriteBuffer,
        S: WriteBuffer,
        I: IntoIterator<Item = (K, Option<V>)>,
        K: WriteBuffer,
        V: WriteBuffer,
    {
        debug_assert!(
            self.state == ResponseState::Headers,
            "Must be called after status() and before any body method"
        );

        name.write_to(&mut self.buffer);
        self.buffer.extend_from_slice(b": ");

        let mut iter = params.into_iter();
        if let Some((first_key, first_val)) = iter.next() {
            first_key.write_to(&mut self.buffer);
            if let Some(val) = first_val {
                self.buffer.extend_from_slice(b"=");
                val.write_to(&mut self.buffer);
            }

            for (key, value) in iter {
                split.write_to(&mut self.buffer);
                key.write_to(&mut self.buffer);
                if let Some(val) = value {
                    self.buffer.extend_from_slice(b"=");
                    val.write_to(&mut self.buffer);
                }
            }
        }

        self.buffer.extend_from_slice(b"\r\n");
        self
    }

    /// Sets the response body and finalizes the response.
    ///
    /// # Side Effects
    /// - Adds a `connection` header if necessary
    /// - Calculates and sets the `content-length` header
    ///
    /// After calling this method, the response is considered complete
    /// and cannot be modified further.
    ///
    /// # Examples
    /// ```
    /// # maker_web::run_test(|_, resp| {
    /// use maker_web::StatusCode;
    //
    /// resp.status(StatusCode::Ok)
    ///     .header("content-type", "text/plain")
    ///     .body("Hello, World!")
    /// # });
    /// ```
    ///
    /// # Panics
    /// Error message: `Must be called after status() and any header methods`
    ///
    /// Panics in `debug` mode when:
    /// - Called before [`status()`](Response::status)
    /// - Called after [`body()`](Response::body) or [`body_with()`](Response::body_with)
    #[inline]
    #[track_caller]
    pub fn body<T: WriteBuffer>(&mut self, data: T) -> Handled {
        debug_assert!(
            self.state == ResponseState::Headers,
            "Must be called after status() and any header methods"
        );

        self.start_body();
        data.write_to(&mut self.buffer);
        self.end_body()
    }

    /// Writes the response body via closure and finalizes the response.
    ///
    /// # Side Effects
    /// - Adds a `connection` header if necessary
    /// - Calculates and sets the `content-length` header
    ///
    /// After calling this method, the response is considered complete
    /// and cannot be modified further.
    ///
    /// # Examples
    /// Using [`write!`]:
    /// ```
    /// # maker_web::run_test(|_, resp| {
    /// use maker_web::StatusCode;
    /// use std::io::Write;
    ///
    /// resp.status(StatusCode::Ok)
    ///     .header("content-type", "application/json")
    ///     .body_with(|writer| {
    ///         // Write JSON directly to the buffer
    ///         write!(writer, r#"{{"status": "ok", "message": "Hello"}}"#);
    ///     })
    /// # });
    /// ```
    /// Using [`WriteBuffer`]:
    /// ```rust
    /// # maker_web::run_test(|_, resp| {
    /// use maker_web::StatusCode;
    ///
    /// resp.status(StatusCode::Ok)
    ///     .header("content-type", "application/octet-stream")
    ///     .body_with(|writer| {
    ///         writer.write(b"lib: ");
    ///         writer.write("maker_web");
    ///         writer.write(b", love_is_lib: ");
    ///         writer.write(true);
    ///         writer.write(b", just_number: ");
    ///         writer.write(123456);
    ///     })
    /// # });
    /// ```
    ///
    /// # Panics
    /// Error message: `Must be called after status() and any header methods`
    ///
    /// Panics in `debug` mode when:
    /// - Called before [`status()`](Response::status)
    /// - Called after [`body()`](Response::body) or [`body_with()`](Response::body_with)
    #[inline]
    #[track_caller]
    pub fn body_with<F: FnOnce(&mut BodyWriter)>(&mut self, f: F) -> Handled {
        debug_assert!(
            self.state == ResponseState::Headers,
            "Must be called after status() and any header methods"
        );

        self.start_body();
        f(&mut BodyWriter(&mut self.buffer));
        self.end_body()
    }
}

impl Response {
    #[inline(always)]
    #[track_caller]
    fn start_body(&mut self) -> &mut Self {
        if let Some(value) = self.connection_header() {
            self.header("connection", value);
        }

        self.buffer.extend_from_slice(b"content-length: ");
        self.posit_length = self.buffer.len();
        self.buffer.extend_from_slice(b"0000000000\r\n\r\n");
        self.start_body = self.buffer.len();
        self
    }

    #[inline(always)]
    fn end_body(&mut self) -> Handled {
        let body_len = self.buffer.len() - self.start_body;
        let (arr, _) = Response::number_to_bytes(body_len as u128);

        let target_range = self.posit_length..self.posit_length + 10;
        self.buffer[target_range].copy_from_slice(&arr[29..39]);
        self.state = ResponseState::Complete;

        Handled(())
    }

    #[inline(always)]
    const fn connection_header(&self) -> Option<&'static [u8]> {
        match (self.version, self.keep_alive) {
            (Version::Http11, true) => None,
            (Version::Http11, false) => Some(b"close"),
            (Version::Http10, true) => Some(b"keep-alive"),
            (Version::Http10, false) => Some(b"close"),
            _ => None,
        }
    }

    #[inline]
    const fn number_to_bytes(mut n: u128) -> ([u8; 39], usize) {
        let mut buffer = [b'0'; 39];
        let mut i = 39;

        if n == 0 {
            return (buffer, 38);
        }

        while n > 0 {
            i -= 1;
            buffer[i] = b'0' + (n % 10) as u8;
            n /= 10;
        }

        (buffer, i)
    }
}

/// Methods for working with `HTTP/0.9+`
///
/// # Connection
/// Automatically closes the connection unless keep-alive path was used.
///
/// # Panics
/// Error messages:
/// - <code>This method is only for \`HTTP/0.9+\`</code>
/// - ``An `HTTP/0.9+` response must use exactly one method``
///
/// In these methods, panic occurs when:
/// - Called in non-`HTTP/0.9+` responses
/// - Calling any method again
impl Response {
    /// Writes a raw `HTTP/0.9+` response and finalizes it.
    ///
    /// # Examples
    /// ```
    /// # maker_web::run_test(|req, resp| {
    /// use maker_web::{StatusCode, Version};
    ///
    /// // For HTTP/0.9+ requests - simple raw response
    /// if req.version() == Version::Http09 {
    ///     resp.http09("user_data_here")
    /// } else {
    ///     resp.status(StatusCode::Ok)
    ///         .header("content-type", "text/plain")
    ///         .body("user_data_here")
    /// }
    /// # });
    /// ```
    /// JSON:
    /// ```
    /// # maker_web::run_test(|req, resp| {
    /// use maker_web::{StatusCode, Version};
    ///
    /// // HTTP/0.9+ with structured data
    /// if req.version() == Version::Http09 {
    ///     resp.http09(r#"{"user_id":123,"name":"John"}"#)
    /// } else {
    ///     resp.status(StatusCode::Ok)
    ///         .header("content-type", "application/json")
    ///         .body(r#"{"user_id":123,"name":"John"}"#)
    /// }
    /// # });
    /// ```
    #[inline]
    #[track_caller]
    pub fn http09<T: WriteBuffer>(&mut self, data: T) -> Handled {
        debug_assert!(
            self.version == Version::Http09,
            "This method is only for `HTTP/0.9+`"
        );
        debug_assert!(
            self.state == ResponseState::Clean,
            "An `HTTP/0.9+` response must use exactly one method"
        );

        data.write_to(&mut self.buffer);
        self.state = ResponseState::Complete;

        Handled(())
    }

    /// Writes `HTTP/0.9+` response via closure and finalizes it.
    ///
    /// # Examples
    /// ```
    /// # maker_web::run_test(|req, resp| {
    /// use maker_web::{StatusCode, Version};
    /// use std::io::Write;
    ///
    /// // Complex HTTP/0.9 response with formatting
    /// if req.version() == Version::Http09 {
    ///     resp.http09_with(|buf| {
    ///         write!(buf, "user_{}_online:{}", 123, true);
    ///     })
    /// } else {
    ///     resp.status(StatusCode::Ok)
    ///         .body_with(|writer| {
    ///             write!(writer, "user_{}_online:{}", 123, true);
    ///         })
    /// }
    /// # });
    /// ```
    /// Bytes data:  
    /// ```
    /// # maker_web::run_test(|req, resp| {
    /// use maker_web::{StatusCode, Version};
    ///
    /// // HTTP/0.9 with binary data
    /// if req.version() == Version::Http09 {
    ///     resp.http09_with(|buf| {
    ///         buf.extend_from_slice(&[0x01, 0x02, 0x03]);
    ///         buf.extend_from_slice(b"payload");
    ///     })
    /// } else {
    ///     resp.status(StatusCode::Ok)
    ///         .header("content-type", "application/octet-stream")
    ///         .body_with(|writer| {
    ///             writer.write(&[0x01, 0x02, 0x03]);
    ///             writer.write(b"payload");
    ///         })
    /// }
    /// # });
    /// ```
    #[inline]
    #[track_caller]
    pub fn http09_with<F: FnOnce(&mut Vec<u8>)>(&mut self, f: F) -> Handled {
        debug_assert!(
            self.version == Version::Http09,
            "This method is only for `HTTP/0.9+`"
        );
        debug_assert!(
            self.state == ResponseState::Clean,
            "An `HTTP/0.9+` response must use exactly one method"
        );

        f(&mut self.buffer);
        self.state = ResponseState::Complete;

        Handled(())
    }

    /// Writes a status code response in `HTTP/0.9+` format and finalizes it.
    ///
    /// Uses semantic prefixes based on status code range:
    /// - `5xx`: `SERVER_ERROR: [code] [reason]\r\n`
    /// - `4xx`: `CLIENT_ERROR: [code] [reason]\r\n`
    /// - `3xx`: `REDIRECT: [code] [reason]\r\n`
    /// - `2xx`: `SUCCESS: [code] [reason]\r\n`
    /// - `1xx`: `INFO: [code] [reason]\r\n`
    ///
    /// # Examples
    /// ```
    /// # maker_web::run_test(|req, resp| {
    /// # let user_exists = true;
    /// use maker_web::{StatusCode, Version};
    ///
    /// // Simple status response for HTTP/0.9+
    /// if req.version() == Version::Http09 {
    ///     if !user_exists {
    ///         return resp.http09_status(StatusCode::NotFound);
    ///     }
    ///     resp.http09("user_data")
    /// } else {
    ///     resp.status(StatusCode::NotFound).body("Not Found")
    /// }
    /// # });
    /// ```
    #[inline]
    #[track_caller]
    pub fn http09_status(&mut self, status: StatusCode) -> Handled {
        debug_assert!(
            self.version == Version::Http09,
            "This method is only for `HTTP/0.9+`"
        );
        debug_assert!(
            self.state == ResponseState::Clean,
            "An `HTTP/0.9+` response must use exactly one method"
        );

        self.buffer
            .extend_from_slice(Self::get_prefix(&status).as_bytes());
        self.buffer
            .extend_from_slice(status.to_first_line(Version::Http09));

        self.state = ResponseState::Complete;
        Handled(())
    }

    /// Writes a custom message response in `HTTP/0.9+` format and finalizes it.
    ///
    /// Uses semantic prefixes based on status code range:
    /// - `5xx`: `SERVER_ERROR: [code] [custom_message]\r\n`
    /// - `4xx`: `CLIENT_ERROR: [code] [custom_message]\r\n`
    /// - `3xx`: `REDIRECT: [code] [custom_message]\r\n`
    /// - `2xx`: `SUCCESS: [code] [custom_message]\r\n`
    /// - `1xx`: `INFO: [code] [custom_message]\r\n`
    ///
    /// # Examples
    /// ```
    /// # maker_web::run_test(|req, resp| {
    /// # let invalid_input = true;
    /// use maker_web::{StatusCode, Version};
    ///
    /// // Custom error message for HTTP/0.9+
    /// if req.version() == Version::Http09 {
    ///     if invalid_input {
    ///         return resp.http09_msg(StatusCode::BadRequest, "Invalid query parameters");
    ///     }
    ///     resp.http09("success")
    /// } else {
    ///     resp.status(StatusCode::BadRequest).body("Invalid query parameters")
    /// }
    /// # });
    /// ```
    /// Success with custom data:
    /// ```
    /// # maker_web::run_test(|req, resp| {
    /// use maker_web::{StatusCode, Version};
    ///
    /// // Success response with structured data
    /// if req.version() == Version::Http09 {
    ///     resp.http09_msg(StatusCode::Ok, r#"{"status":"ok","id":12345}"#)
    /// } else {
    ///     resp.status(StatusCode::Ok)
    ///         .header("content-type", "application/json")
    ///         .body(r#"{"status":"ok","id":12345}"#)
    /// }
    /// # });
    /// ```
    #[inline]
    #[track_caller]
    pub fn http09_msg<T: WriteBuffer>(&mut self, status: StatusCode, value: T) -> Handled {
        debug_assert!(
            self.version == Version::Http09,
            "This method is only for `HTTP/0.9+`"
        );
        debug_assert!(
            self.state == ResponseState::Clean,
            "An `HTTP/0.9+` response must use exactly one method"
        );

        self.buffer
            .extend_from_slice(Self::get_prefix(&status).as_bytes());
        self.buffer.extend_from_slice(status.as_u16_bytes());
        value.write_to(&mut self.buffer);
        self.buffer.extend_from_slice(b"\r\n");

        self.state = ResponseState::Complete;
        Handled(())
    }

    #[inline]
    const fn get_prefix(status: &StatusCode) -> &str {
        match *status as u16 {
            400..=499 => "CLIENT_ERROR:",
            500..=599 => "SERVER_ERROR:",
            300..=399 => "REDIRECT:",
            200..=299 => "SUCCESS:",
            100..=199 => "INFO:",
            _ => "?:",
        }
    }
}

pub mod write {
    use super::*;

    /// Writer for constructing the HTTP response body.
    /// Used in [body_with](Response::body_with).
    ///
    /// # Examples
    ///
    /// With [WriteBuffer]:
    /// ```
    /// # maker_web::run_test(|_, resp| {
    /// use maker_web::StatusCode;
    ///
    /// resp.status(StatusCode::Ok)
    ///     .body_with(|w| {
    ///         w.write("This goes in the response body");
    ///         w.write("<html>...</html>");
    ///         w.write(true);
    ///     })
    /// # });
    /// ```
    /// With [std::io::Write]:
    /// ```
    /// # maker_web::run_test(|_, resp| {
    /// use maker_web::StatusCode;
    /// use std::io::Write;
    ///
    /// resp.status(StatusCode::Ok)
    ///     .body_with(|w| {
    ///         write!(w, "This goes in the response body");
    ///         write!(w, "{} - {} = {}", 6, 2, 4);
    ///     })
    /// # });
    /// ```
    #[derive(Debug)]
    pub struct BodyWriter<'a>(pub(crate) &'a mut Vec<u8>);

    impl BodyWriter<'_> {
        /// Appends content to the response body.
        ///
        /// Adds data to the body section of the HTTP response. This method
        /// only affects the response body, not headers or status.
        ///
        /// # Examples
        /// ```
        /// # maker_web::run_test(|_, resp| {
        /// use maker_web::StatusCode;
        ///
        /// resp.status(StatusCode::Ok)
        ///     .body_with(|w| {
        ///         w.write("Hello");
        ///         w.write(123);
        ///         w.write(true);
        ///     })
        /// # });
        /// ```
        #[inline]
        pub fn write<T: WriteBuffer>(&mut self, value: T) {
            value.write_to(self.0);
        }
    }

    impl std::io::Write for BodyWriter<'_> {
        #[inline]
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.extend_from_slice(buf);
            Ok(buf.len())
        }

        #[inline]
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Trait for writing data to the [`Response`] buffer.
    ///
    /// Implemented for common types like strings, bytes, booleans
    /// and numeric types (excluding floating-point numbers)
    ///
    /// # Note on Floating-Point
    /// Floating-point numbers are not implemented to avoid locale-dependent
    /// formatting and precision issues in protocol headers.
    ///
    /// For explicit float serialization, consider using the
    /// [`ryu`](https://crates.io/crates/ryu)
    /// crate or formatting to string with controlled precision.
    ///
    /// # Example
    /// ```
    /// use maker_web::WriteBuffer;
    ///
    /// struct MyString(String);
    ///
    /// impl WriteBuffer for MyString {
    ///     fn write_to(&self, buffer: &mut Vec<u8>) {
    ///         buffer.extend_from_slice(self.0.as_bytes())
    ///     }
    /// }
    /// ```
    pub trait WriteBuffer {
        /// Writes the value's representation directly to the buffer.
        ///
        /// This should avoid intermediate allocations and write the
        /// most efficient representation possible.
        fn write_to(&self, buffer: &mut Vec<u8>);
    }

    macro_rules! impl_write_buffer {
        (bytes, $conn:expr => $($t:ty),*) => {
            $(impl WriteBuffer for $t {
                #[inline] fn write_to(&self, buffer: &mut Vec<u8>) {
                    let closure = $conn;
                    closure(self, buffer);
                }
            })*
        };
        (number($type:ty), $conn:expr => $($t:ty),*) => {
            $(impl WriteBuffer for $t {
                #[inline] fn write_to(&self, buffer: &mut Vec<u8>) {
                    $conn(*self as $type, buffer);
                }
            })*
        };
        (non_zero($type:ty), $conn:expr => $($t:ident),*) => {
            $(impl WriteBuffer for std::num::$t {
                #[inline] fn write_to(&self, buffer: &mut Vec<u8>) {
                    $conn(self.get() as $type, buffer);
                }
            })*
        };
    }

    impl<T: WriteBuffer> WriteBuffer for &T {
        #[inline]
        fn write_to(&self, buffer: &mut Vec<u8>) {
            T::write_to(*self, buffer);
        }
    }
    impl<T: WriteBuffer> WriteBuffer for &mut T {
        #[inline]
        fn write_to(&self, buffer: &mut Vec<u8>) {
            T::write_to(*self, buffer);
        }
    }
    impl_write_buffer! {
        bytes, |value: &str, buffer: &mut Vec<u8>| {
            buffer.extend_from_slice(value.as_bytes());
        } => &str, String, Box<str>, Cow<'_, str>,
        Arc<str>, Rc<str>, Arc<String>, Rc<String>
    }
    impl_write_buffer! {
        bytes, |value: &[u8], buffer: &mut Vec<u8>| {
            buffer.extend_from_slice(value);
        } => &[u8], Vec<u8>, Box<[u8]>, Cow<'_, [u8]>,
        Arc<[u8]>, Rc<[u8]>, Arc<Vec<u8>>, Rc<Vec<u8>>
    }
    impl<const N: usize> WriteBuffer for [u8; N] {
        #[inline]
        fn write_to(&self, buffer: &mut Vec<u8>) {
            buffer.extend_from_slice(self);
        }
    }
    impl_write_buffer! {
        number(u128), impl_write_buffer_u128 => u8, u16, u32, u64, u128, usize
    }
    impl_write_buffer! {
        non_zero(u128), impl_write_buffer_u128 => NonZeroU8,
        NonZeroU16, NonZeroU32, NonZeroU64, NonZeroU128, NonZeroUsize
    }
    impl_write_buffer! {
        number(i128), impl_write_buffer_i128 => i8, i16, i32, i64, i128, isize
    }
    impl_write_buffer! {
        non_zero(i128), impl_write_buffer_i128 => NonZeroI8,
        NonZeroI16, NonZeroI32, NonZeroI64, NonZeroI128, NonZeroIsize
    }
    impl WriteBuffer for bool {
        #[inline]
        fn write_to(&self, buffer: &mut Vec<u8>) {
            buffer.extend_from_slice(match self {
                true => b"true",
                false => b"false",
            });
        }
    }
    impl WriteBuffer for char {
        #[inline]
        fn write_to(&self, buffer: &mut Vec<u8>) {
            let mut buf = [0u8; 4];
            buffer.extend_from_slice(self.encode_utf8(&mut buf).as_bytes());
        }
    }

    #[inline(always)]
    fn impl_write_buffer_u128(value: u128, buffer: &mut Vec<u8>) {
        let (arr, start) = Response::number_to_bytes(value);
        buffer.extend_from_slice(&arr[start..]);
    }

    #[inline(always)]
    fn impl_write_buffer_i128(value: i128, buffer: &mut Vec<u8>) {
        if value < 0 {
            buffer.push(b'-');
        }
        let abs = value.unsigned_abs();

        let (arr, start) = Response::number_to_bytes(abs);
        buffer.extend_from_slice(&arr[start..]);
    }
}

#[cfg(test)]
mod close_tests {
    use super::*;
    use crate::tools::*;

    #[test]
    fn basic() {
        let cases = [
            (Version::Http11, false, ""),
            (Version::Http11, true, "connection: close\r\n"),
            (Version::Http10, false, "connection: keep-alive\r\n"),
            (Version::Http10, true, "connection: close\r\n"),
        ];

        for (version, is_close, header) in cases {
            let mut resp = Response::new(&RespLimits::default());
            resp.version = version;

            assert_eq!(resp.keep_alive, true);
            if is_close {
                resp.close();
                assert_eq!(resp.keep_alive, false);
                resp.close();
                assert_eq!(resp.keep_alive, false);
            }

            resp.status(StatusCode::Ok).body("");
            assert_eq!(
                str_op(&resp.buffer),
                format!(
                    "{}{header}content-length: 0000000000\r\n\r\n",
                    str_op(StatusCode::Ok.to_first_line(version))
                )
            );
        }
    }

    #[test]
    #[should_panic(expected = "Must be called before any finalizing method")]
    fn after_body() {
        let mut resp = Response::new(&RespLimits::default());
        resp.status(StatusCode::Ok).body("");
        resp.close();
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;
    use crate::tools::*;

    #[test]
    fn basic() {
        let cases = [
            (StatusCode::Ok, "HTTP/1.1 200 OK\r\n"),
            (StatusCode::NotFound, "HTTP/1.1 404 Not Found\r\n"),
            (StatusCode::Found, "HTTP/1.1 302 Found\r\n"),
            (StatusCode::BadRequest, "HTTP/1.1 400 Bad Request\r\n"),
        ];

        for (status, result) in cases {
            let mut resp = Response::new(&RespLimits::default());

            assert_eq!(resp.buffer, []);
            assert_eq!(resp.state, ResponseState::Clean);

            resp.status(status);
            assert_eq!(str_op(&resp.buffer), result);
            assert_eq!(resp.state, ResponseState::Headers);
        }
    }

    #[test]
    #[should_panic(expected = "Must be first and called only once")]
    fn double_call() {
        Response::new(&RespLimits::default())
            .status(StatusCode::Ok)
            .status(StatusCode::Found);
    }

    #[test]
    #[should_panic(expected = "This method is only for `HTTP/1.X`")]
    fn http09_panic() {
        let mut resp = Response::new(&RespLimits::default());
        resp.version = Version::Http09;

        resp.status(StatusCode::Ok);
    }
}

#[cfg(test)]
mod header_tests {
    use super::*;
    use crate::tools::*;

    macro_rules! test_header {
        ($method:ident, $(($name:expr $(, $params:expr)*; $result:expr);)*) => {
           #[test] fn $method() {$(
            let mut resp = Response::new(&RespLimits::default());

            assert_eq!(resp.buffer, []);

            resp.status(StatusCode::Ok);
            assert_eq!(resp.state, ResponseState::Headers);

            resp.$method($name $(, $params)*);
            assert_eq!(str_op(&resp.buffer[17..]), $result);
            assert_eq!(resp.state, ResponseState::Headers);
        )*}};
    }

    test_header! {header,
        ("name", "value"; "name: value\r\n");
        ("", "value"; ": value\r\n");
        ("name", ""; "name: \r\n");

        ("name", 123; "name: 123\r\n");
        ("name", vec![35, 33, 43]; "name: #!+\r\n");
        ("name", false; "name: false\r\n");
        ("name", -123; "name: -123\r\n");
    }

    #[test]
    #[should_panic(expected = "Must be called after status() and before any body method")]
    fn header_before_status() {
        Response::new(&RespLimits::default()).header("Name", "value");
    }

    #[test]
    #[should_panic(expected = "Must be called after status() and before any body method")]
    fn header_after_body() {
        let mut resp = Response::new(&RespLimits::default());
        resp.status(StatusCode::Ok).body("");
        resp.header("Name", "value");
    }

    test_header! {header_multi,
        ("name", ", ", ["q", "w", "e"]; "name: q, w, e\r\n");
        ("name", ",", [true, false]; "name: true,false\r\n");
        ("name", "; ", [-123, 123]; "name: -123; 123\r\n");
    }

    #[test]
    #[should_panic(expected = "Must be called after status() and before any body method")]
    fn header_multi_before_status() {
        Response::new(&RespLimits::default()).header_multi("Name", ",", ["value1", "value2"]);
    }

    #[test]
    #[should_panic(expected = "Must be called after status() and before any body method")]
    fn header_multi_after_body() {
        let mut resp = Response::new(&RespLimits::default());
        resp.status(StatusCode::Ok).body("");
        resp.header_multi("Name", ",", ["value1", "value2"]);
    }

    test_header! {header_params,
        ("name", ", ", [("name", Some("value"))]; "name: name=value\r\n");
        (
            "name", ", ", [("q", Some("1")), ("w", Some("2")), ("e", Some("3"))];
            "name: q=1, w=2, e=3\r\n"
        );
        (
            "name", ";", [("q", Some("v1")), ("w", Some("v2")), ("e", Some("v3"))];
            "name: q=v1;w=v2;e=v3\r\n"
        );
        (
            "name", ", ", [("min", Some(-128)), ("max", Some(128)), ("mean", Some(0))];
            "name: min=-128, max=128, mean=0\r\n"
        );
        (
            "u128", ", ", [("min", Some(u128::MIN)), ("max", Some(u128::MAX))];
            "u128: min=0, max=340282366920938463463374607431768211455\r\n"
        );
        (
            "i128", ", ", [("min", Some(i128::MIN)), ("max", Some(i128::MAX))];
    "i128: min=-170141183460469231731687303715884105728, max=170141183460469231731687303715884105727\r\n"
        );
        (
            "name", ", ", [("debug", Some(true)), ("doc", Some(false))];
            "name: debug=true, doc=false\r\n"
        );
        (
            "name", "; ", [("debug", None), ("text", Some("asd"))];
            "name: debug; text=asd\r\n"
        );
    }

    #[test]
    #[should_panic(expected = "Must be called after status() and before any body method")]
    fn header_params_before_status() {
        Response::new(&RespLimits::default()).header_params(
            "Name",
            ",",
            [("name1", Some("value1")), ("name2", None)],
        );
    }

    #[test]
    #[should_panic(expected = "Must be called after status() and before any body method")]
    fn header_params_after_body() {
        let mut resp = Response::new(&RespLimits::default());
        resp.status(StatusCode::Ok).body("");
        resp.header_params("Name", ",", [("name1", Some("value1")), ("name2", None)]);
    }
}

#[cfg(test)]
mod body_tests {
    use super::*;
    use crate::tools::*;

    macro_rules! test_body {
        ($method:ident, $(($data:expr, $len:expr);)*) => {
        #[test] fn $method() {$(
            let mut resp = Response::new(&RespLimits::default());

            let result_data = test_body!{ $method, resp, $data };

            assert_eq!(
                str_op(&resp.buffer),
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{}",
                    format!("{:0>10}", $len),
                    str_op(&result_data)
                )
            );
            assert_eq!(resp.state, ResponseState::Complete);
        )*}};

        (body, $resp:expr, $data:expr) => {{
            $resp.status(StatusCode::Ok).body($data);
            let mut expected = Vec::new();
            $data.write_to(&mut expected);
            expected
        }};
        (body_with, $resp:expr, $data:expr) => {{
            $resp.status(StatusCode::Ok).body_with($data);

            let mut vector = Vec::new();
            let mut result_data = BodyWriter(&mut vector);
            $data(&mut result_data);
            vector
        }};
    }

    test_body! {body,
        ("sample body", 11);
        ("{\"debug\": true, \"doc\": false}", 29);
        (true, 4);
        (-1234, 5);
    }

    #[test]
    #[should_panic(expected = "Must be called after status() and any header methods")]
    fn body_before_status() {
        Response::new(&RespLimits::default()).body("Name");
    }

    #[test]
    #[should_panic(expected = "Must be called after status() and any header methods")]
    fn body_double_call() {
        let mut resp = Response::new(&RespLimits::default());
        resp.status(StatusCode::Ok).body("");
        resp.body("Name");
    }

    test_body! {body_with,
        (|buf: &mut BodyWriter| buf.write("qwe"), 3);
        (|buf: &mut BodyWriter| buf.write(vec![23, 34, 56]), 3);
        (|buf: &mut BodyWriter| buf.write(String::from("body")), 4);
    }

    #[test]
    #[should_panic(expected = "Must be called after status() and any header methods")]
    fn body_with_before_status() {
        Response::new(&RespLimits::default()).body_with(|_| {});
    }

    #[test]
    #[should_panic(expected = "Must be called after status() and any header methods")]
    fn body_with_double_call() {
        let mut resp = Response::new(&RespLimits::default());
        resp.status(StatusCode::Ok).body_with(|_| {});
        resp.body_with(|_| {});
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::tools::*;

    #[test]
    fn full_sequence_with_close() {
        let mut resp = Response::new(&RespLimits::default());
        let result = [
            "HTTP/1.1 302 Found\r\n",
            "HTTP/1.1 302 Found\r\nlocation: /api/update\r\n",
            "connection: close\r\ncontent-length: 0000000011\r\n\r\nSample body",
        ];

        assert_eq!(resp.buffer, []);
        assert_eq!(resp.state, ResponseState::Clean);

        resp.status(StatusCode::Found);
        assert_eq!(str_op(&resp.buffer), result[0]);
        assert_eq!(resp.state, ResponseState::Headers);

        resp.header("location", "/api/update");
        assert_eq!(str_op(&resp.buffer), result[1]);
        assert_eq!(resp.state, ResponseState::Headers);

        assert_eq!(resp.keep_alive, true);
        resp.close();
        assert_eq!(resp.keep_alive, false);
        assert_eq!(str_op(&resp.buffer), result[1]);
        assert_eq!(resp.state, ResponseState::Headers);

        resp.body("Sample body");
        assert_eq!(str_op(&resp.buffer), result[1].to_owned() + result[2]);
        assert_eq!(resp.state, ResponseState::Complete);
    }

    #[test]
    fn full_sequence() {
        let mut resp = Response::new(&RespLimits::default());
        let result = [
            "HTTP/1.1 302 Found\r\n",
            "HTTP/1.1 302 Found\r\nlocation: /api/update\r\n",
            "content-length: 0000000011\r\n\r\nSample body",
        ];

        assert_eq!(resp.buffer, []);
        assert_eq!(resp.state, ResponseState::Clean);

        resp.status(StatusCode::Found);
        assert_eq!(str_op(&resp.buffer), result[0]);
        assert_eq!(resp.state, ResponseState::Headers);

        resp.header("location", "/api/update");
        assert_eq!(str_op(&resp.buffer), result[1]);
        assert_eq!(resp.state, ResponseState::Headers);

        resp.body("Sample body");
        assert_eq!(str_op(&resp.buffer), result[1].to_owned() + result[2]);
        assert_eq!(resp.state, ResponseState::Complete);
    }

    #[test]
    fn minimal_sequence_with_close() {
        let mut resp = Response::new(&RespLimits::default());
        let result = [
            "HTTP/1.1 302 Found\r\n",
            "connection: close\r\ncontent-length: 0000000011\r\n\r\nSample body",
        ];

        assert_eq!(resp.buffer, []);
        assert_eq!(resp.state, ResponseState::Clean);

        resp.status(StatusCode::Found);
        assert_eq!(str_op(&resp.buffer), result[0]);
        assert_eq!(resp.state, ResponseState::Headers);

        assert_eq!(resp.keep_alive, true);
        resp.close();
        assert_eq!(resp.keep_alive, false);
        assert_eq!(str_op(&resp.buffer), result[0]);
        assert_eq!(resp.state, ResponseState::Headers);

        resp.body("Sample body");
        assert_eq!(str_op(&resp.buffer), result[0].to_owned() + result[1]);
        assert_eq!(resp.state, ResponseState::Complete);
    }

    #[test]
    fn minimal_sequence() {
        let mut resp = Response::new(&RespLimits::default());
        let result = [
            "HTTP/1.1 302 Found\r\n",
            "content-length: 0000000011\r\n\r\nSample body",
        ];

        assert_eq!(resp.buffer, []);
        assert_eq!(resp.state, ResponseState::Clean);

        resp.status(StatusCode::Found);
        assert_eq!(str_op(&resp.buffer), result[0]);
        assert_eq!(resp.state, ResponseState::Headers);

        resp.body("Sample body");
        assert_eq!(str_op(&resp.buffer), result[0].to_owned() + result[1]);
        assert_eq!(resp.state, ResponseState::Complete);
    }
}

#[cfg(test)]
mod http09_tests {
    use super::*;
    use crate::tools::*;

    #[test]
    fn basic() {
        let result = "just text, just to have it :)";

        let mut resp = Response::new(&RespLimits::default());
        resp.version = Version::Http09;

        assert_eq!(resp.buffer, []);
        assert_eq!(resp.state, ResponseState::Clean);

        resp.http09(result);
        assert_eq!(str_op(&resp.buffer), result);
        assert_eq!(resp.state, ResponseState::Complete);
    }

    #[test]
    #[should_panic(expected = "An `HTTP/0.9+` response must use exactly one method")]
    fn double_call() {
        let mut resp = Response::new(&RespLimits::default());
        resp.version = Version::Http09;

        resp.http09("Call number 1");
        resp.http09("Call number 2 :)");
    }

    #[test]
    #[should_panic(expected = "This method is only for `HTTP/0.9+`")]
    fn http1x_panic() {
        Response::new(&RespLimits::default()).http09("just text");
    }
}

#[cfg(test)]
mod http09_with_tests {
    use super::*;
    use crate::tools::*;

    #[test]
    fn basic() {
        let mut resp = Response::new(&RespLimits::default());
        resp.version = Version::Http09;

        assert_eq!(resp.buffer, []);
        assert_eq!(resp.state, ResponseState::Clean);

        resp.http09_with(|buf| {
            true.write_to(buf);
            "; ".write_to(buf);
            123.write_to(buf);
            "; ".write_to(buf);
            [34, 35, 36].write_to(buf);
        });
        assert_eq!(str_op(&resp.buffer), "true; 123; \"#$");
        assert_eq!(resp.state, ResponseState::Complete);
    }

    #[test]
    #[should_panic(expected = "An `HTTP/0.9+` response must use exactly one method")]
    fn double_call() {
        let mut resp = Response::new(&RespLimits::default());
        resp.version = Version::Http09;

        resp.http09_with(|_| {});
        resp.http09_with(|_| {});
    }

    #[test]
    #[should_panic(expected = "This method is only for `HTTP/0.9+`")]
    fn http1x_panic() {
        Response::new(&RespLimits::default()).http09_with(|_| {});
    }
}

#[cfg(test)]
mod http09_status_tests {
    use super::*;
    use crate::tools::*;

    #[test]
    fn basic() {
        let cases = [
            (StatusCode::Continue, "INFO: 100 Continue\r\n"),
            (StatusCode::Ok, "SUCCESS: 200 OK\r\n"),
            (
                StatusCode::MultipleChoices,
                "REDIRECT: 300 Multiple Choices\r\n",
            ),
            (StatusCode::BadRequest, "CLIENT_ERROR: 400 Bad Request\r\n"),
            (
                StatusCode::InternalServerError,
                "SERVER_ERROR: 500 Internal Server Error\r\n",
            ),
        ];

        for (status, result) in cases {
            let mut resp = Response::new(&RespLimits::default());
            resp.version = Version::Http09;

            assert_eq!(resp.buffer, []);
            assert_eq!(resp.state, ResponseState::Clean);

            resp.http09_status(status);
            assert_eq!(str_op(&resp.buffer), result);
            assert_eq!(resp.state, ResponseState::Complete);
        }
    }

    #[test]
    #[should_panic(expected = "An `HTTP/0.9+` response must use exactly one method")]
    fn double_call() {
        let mut resp = Response::new(&RespLimits::default());
        resp.version = Version::Http09;

        resp.http09_status(StatusCode::Ok);
        resp.http09_status(StatusCode::Found);
    }

    #[test]
    #[should_panic(expected = "This method is only for `HTTP/0.9+`")]
    fn http1x_panic() {
        Response::new(&RespLimits::default()).http09_status(StatusCode::Ok);
    }
}

#[cfg(test)]
mod http09_msg_tests {
    use super::*;
    use crate::tools::*;

    #[test]
    fn basic() {
        let cases = [
            (
                StatusCode::Continue,
                "sample message 1",
                "INFO: 100 sample message 1\r\n",
            ),
            (
                StatusCode::Ok,
                "sample message 2",
                "SUCCESS: 200 sample message 2\r\n",
            ),
            (
                StatusCode::MultipleChoices,
                "sample message 3",
                "REDIRECT: 300 sample message 3\r\n",
            ),
            (
                StatusCode::BadRequest,
                "sample message 4",
                "CLIENT_ERROR: 400 sample message 4\r\n",
            ),
            (
                StatusCode::InternalServerError,
                "sample message 5",
                "SERVER_ERROR: 500 sample message 5\r\n",
            ),
        ];

        for (status, value, result) in cases {
            let mut resp = Response::new(&RespLimits::default());
            resp.version = Version::Http09;

            assert_eq!(resp.buffer, []);
            assert_eq!(resp.state, ResponseState::Clean);

            resp.http09_msg(status, value);
            assert_eq!(str_op(&resp.buffer), result);
            assert_eq!(resp.state, ResponseState::Complete);
        }
    }

    #[test]
    #[should_panic(expected = "An `HTTP/0.9+` response must use exactly one method")]
    fn double_call() {
        let mut resp = Response::new(&RespLimits::default());
        resp.version = Version::Http09;

        resp.http09_msg(StatusCode::Ok, "");
        resp.http09_msg(StatusCode::Found, "");
    }

    #[test]
    #[should_panic(expected = "This method is only for `HTTP/0.9+`")]
    fn http1x_panic() {
        Response::new(&RespLimits::default()).http09_msg(StatusCode::Ok, "");
    }
}
