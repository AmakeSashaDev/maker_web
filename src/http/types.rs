#![allow(rustdoc::bare_urls)]

//! Core HTTP protocol types and utilities

use crate::{errors::ErrorKind, limits::ReqLimits};
use std::mem;

#[inline(always)]
pub(crate) fn slice_to_usize(bytes: &[u8]) -> Option<usize> {
    let mut result: usize = 0;

    for &byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }

        result = result
            .checked_mul(10)?
            .checked_add((byte - b'0') as usize)?;
    }

    Some(result)
}

// METHOD

/// HTTP request methods
///
/// # References
///
/// - [RFC 7231, Section 4](https://datatracker.ietf.org/doc/html/rfc7231#section-4)
/// - [RFC 5789](https://datatracker.ietf.org/doc/html/rfc5789) (PATCH method)
///
/// # Disabled methods
///
/// * `TRACE` - disabled for security reasons
/// * `CONNECT` - disabled because it is no longer needed
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Method {
    /// `GET` method - transfer a current representation of the target resource
    /// [[RFC7231, Section 4.3.1](https://tools.ietf.org/html/rfc7231#section-4.3.1)]
    Get,
    /// `PUT` method - replace all current representations of the target resource with the request payload
    /// [[RFC7231, Section 4.3.4](https://tools.ietf.org/html/rfc7231#section-4.3.4)]
    Put,
    /// `POST` method - perform resource-specific processing on the request payload
    /// [[RFC7231, Section 4.3.3](https://tools.ietf.org/html/rfc7231#section-4.3.3)]
    Post,
    /// `HEAD` method - same as GET but without response body
    /// [[RFC7231, Section 4.3.2](https://tools.ietf.org/html/rfc7231#section-4.3.2)]
    Head,
    /// `PATCH` method - apply partial modifications to a resource
    /// [[RFC5789, Section 2](https://tools.ietf.org/html/rfc5789#section-2)]
    Patch,
    /// `DELETE` method - remove all current representations of the target resource
    /// [[RFC7231, Section 4.3.5](https://tools.ietf.org/html/rfc7231#section-4.3.5)]
    Delete,
    /// `OPTIONS` method - describe the communication options for the target resource
    /// [[RFC7231, Section 4.3.7](https://tools.ietf.org/html/rfc7231#section-4.3.7)]
    Options,
}

impl Method {
    #[inline]
    pub(crate) const fn from_bytes(src: &[u8]) -> Result<Self, ErrorKind> {
        match src {
            b"GET" => Ok(Method::Get),
            b"PUT" => Ok(Method::Put),
            b"POST" => Ok(Method::Post),
            b"HEAD" => Ok(Method::Head),
            b"PATCH" => Ok(Method::Patch),
            b"DELETE" => Ok(Method::Delete),
            b"OPTIONS" => Ok(Method::Options),
            _ => Err(ErrorKind::InvalidMethod),
        }
    }

    #[inline]
    pub const fn as_str(&self) -> &str {
        match self {
            Method::Get => "GET",
            Method::Put => "PUT",
            Method::Post => "POST",
            Method::Head => "HEAD",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
            Method::Options => "OPTIONS",
        }
    }
}

// VERSION

/// HTTP protocol version
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Version {
    /// HTTP/0.9 - The original protocol (1991)
    ///
    /// Minimalist format: `GET /path\r\n` with raw response body.
    /// Used for maximum performance in microservice communication.
    ///
    /// [Original specification](https://www.w3.org/Protocols/HTTP/AsImplemented.html)
    Http09,

    /// HTTP/1.0 - Added headers and status codes (1996)  
    ///
    /// [RFC 1945](https://tools.ietf.org/html/rfc1945)
    Http10,

    /// HTTP/1.1 - Current standard with keep-alive and chunking (1999)
    ///
    /// [RFC 7230](https://tools.ietf.org/html/rfc7230) and related
    Http11,
}

impl Version {
    #[inline]
    pub const fn as_str(&self) -> &str {
        match self {
            Version::Http11 => "HTTP/1.1",
            Version::Http10 => "HTTP/1.0",
            Version::Http09 => "HTTP/0.9+",
        }
    }
}

// STATUS_CODE

macro_rules! set_status_codes {
    ($(
        $(#[$docs:meta])+
        $name:ident = ($num:expr, $str:expr);
    )+) => {
        /// HTTP status codes
        ///
        /// Represents valid HTTP status codes as defined in
        /// [RFC 7231](https://tools.ietf.org/html/rfc7231#section-6) and other standards.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum StatusCode { $(
            #[doc = concat!(stringify!($num), " ", $str)]
            $(#[$docs])+
            $name = $num,
        )+ }

        impl StatusCode {
            // Returns the HTTP first line as bytes (e.g., `b"HTTP/1.1 200 OK\r\n"`).
            #[inline]
            pub(crate) const fn to_first_line(self, version: Version) -> &'static [u8] {
                match (self, version) { $(
                    (StatusCode::$name, Version::Http11) => {
                        concat!("HTTP/1.1 ", $num, " ", $str, "\r\n").as_bytes()
                    },
                    (StatusCode::$name, Version::Http10) => {
                        concat!("HTTP/1.0 ", $num, " ", $str, "\r\n").as_bytes()
                    },
                    (StatusCode::$name, Version::Http09) => {
                        concat!(" ", $num, " ", $str, "\r\n").as_bytes()
                    },
                )+ }
            }

            #[inline]
            pub(crate) const fn as_u16_bytes(&self) -> &[u8] {
                match self { $(
                    StatusCode::$name => concat!(" ", $num, " ").as_bytes(),
                )+ }
            }

            #[inline]
            pub const fn as_str(&self) -> &str {
                match self { $(
                    StatusCode::$name => concat!($num, " ", $str),
                )+ }
            }
        }
    }
}

set_status_codes! {
    /// [[RFC9110, Section 15.2.1](https://datatracker.ietf.org/doc/html/rfc9110#section-15.2.1)]
    Continue = (100, "Continue");
    /// [[RFC9110, Section 15.2.2](https://datatracker.ietf.org/doc/html/rfc9110#section-15.2.2)]
    SwitchingProtocols = (101, "Switching Protocols");
    /// [[RFC2518, Section 10.1](https://datatracker.ietf.org/doc/html/rfc2518#section-10.1)]
    Processing = (102, "Processing");

    /// [[RFC9110, Section 15.3.1](https://datatracker.ietf.org/doc/html/rfc9110#section-15.3.1)]
    Ok = (200, "OK");
    /// [[RFC9110, Section 15.3.2](https://datatracker.ietf.org/doc/html/rfc9110#section-15.3.2)]
    Created = (201, "Created");
    /// [[RFC9110, Section 15.3.3](https://datatracker.ietf.org/doc/html/rfc9110#section-15.3.3)]
    Accepted = (202, "Accepted");
    /// [[RFC9110, Section 15.3.4](https://datatracker.ietf.org/doc/html/rfc9110#section-15.3.4)]
    NonAuthoritativeInformation = (203, "Non Authoritative Information");
    /// [[RFC9110, Section 15.3.5](https://datatracker.ietf.org/doc/html/rfc9110#section-15.3.5)]
    NoContent = (204, "No Content");
    /// [[RFC9110, Section 15.3.6](https://datatracker.ietf.org/doc/html/rfc9110#section-15.3.6)]
    ResetContent = (205, "Reset Content");
    /// [[RFC9110, Section 15.3.7](https://datatracker.ietf.org/doc/html/rfc9110#section-15.3.7)]
    PartialContent = (206, "Partial Content");
    /// [[RFC4918, Section 11.1](https://datatracker.ietf.org/doc/html/rfc4918#section-11.1)]
    MultiStatus = (207, "Multi-Status");
    /// [[RFC5842, Section 7.1](https://datatracker.ietf.org/doc/html/rfc5842#section-7.1)]
    AlreadyReported = (208, "Already Reported");
    /// [[RFC3229, Section 10.4.1](https://datatracker.ietf.org/doc/html/rfc3229#section-10.4.1)]
    ImUsed = (226, "IM Used");

    /// [[RFC9110, Section 15.4.1](https://datatracker.ietf.org/doc/html/rfc9110#section-15.4.1)]
    MultipleChoices = (300, "Multiple Choices");
    /// [[RFC9110, Section 15.4.2](https://datatracker.ietf.org/doc/html/rfc9110#section-15.4.2)]
    MovedPermanently = (301, "Moved Permanently");
    /// [[RFC9110, Section 15.4.3](https://datatracker.ietf.org/doc/html/rfc9110#section-15.4.3)]
    Found = (302, "Found");
    /// [[RFC9110, Section 15.4.4](https://datatracker.ietf.org/doc/html/rfc9110#section-15.4.4)]
    SeeOther = (303, "See Other");
    /// [[RFC9110, Section 15.4.5](https://datatracker.ietf.org/doc/html/rfc9110#section-15.4.5)]
    NotModified = (304, "Not Modified");
    /// [[RFC9110, Section 15.4.6](https://datatracker.ietf.org/doc/html/rfc9110#section-15.4.6)]
    UseProxy = (305, "Use Proxy");
    /// [[RFC9110, Section 15.4.7](https://datatracker.ietf.org/doc/html/rfc9110#section-15.4.7)]
    TemporaryRedirect = (307, "Temporary Redirect");
    /// [[RFC9110, Section 15.4.8](https://datatracker.ietf.org/doc/html/rfc9110#section-15.4.8)]
    PermanentRedirect = (308, "Permanent Redirect");

    /// [[RFC9110, Section 15.5.1](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.1)]
    BadRequest = (400, "Bad Request");
    /// [[RFC9110, Section 15.5.2](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.2)]
    Unauthorized = (401, "Unauthorized");
    /// [[RFC9110, Section 15.5.3](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.3)]
    PaymentRequired = (402, "Payment Required");
    /// [[RFC9110, Section 15.5.4](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.4)]
    Forbidden = (403, "Forbidden");
    /// [[RFC9110, Section 15.5.5](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.5)]
    NotFound = (404, "Not Found");
    /// [[RFC9110, Section 15.5.6](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.6)]
    MethodNotAllowed = (405, "Method Not Allowed");
    /// [[RFC9110, Section 15.5.7](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.7)]
    NotAcceptable = (406, "Not Acceptable");
    /// [[RFC9110, Section 15.5.8](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.8)]
    ProxyAuthenticationRequired = (407, "Proxy Authentication Required");
    /// [[RFC9110, Section 15.5.9](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.9)]
    RequestTimeout = (408, "Request Timeout");
    /// [[RFC9110, Section 15.5.10](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.10)]
    Conflict = (409, "Conflict");
    /// [[RFC9110, Section 15.5.11](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.11)]
    Gone = (410, "Gone");
    /// [[RFC9110, Section 15.5.12](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.12)]
    LengthRequired = (411, "Length Required");
    /// [[RFC9110, Section 15.5.13](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.13)]
    PreconditionFailed = (412, "Precondition Failed");
    /// [[RFC9110, Section 15.5.14](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.14)]
    PayloadTooLarge = (413, "Payload Too Large");
    /// [[RFC9110, Section 15.5.15](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.15)]
    UriTooLong = (414, "URI Too Long");
    /// [[RFC9110, Section 15.5.16](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.16)]
    UnsupportedMediaType = (415, "Unsupported Media Type");
    /// [[RFC9110, Section 15.5.17](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.17)]
    RangeNotSatisfiable = (416, "Range Not Satisfiable");
    /// [[RFC9110, Section 15.5.18](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.18)]
    ExpectationFailed = (417, "Expectation Failed");
    /// [Originally RFC 2324](https://datatracker.ietf.org/doc/html/rfc2324#section-2.3.2),
    /// now [RFC9110, Section 15.5.19](https://datatracker.ietf.org/doc/html/rfc9110#name-418-unused),
    /// [reserved by IANA](https://www.iana.org/assignments/http-status-codes/http-status-codes.xhtml).
    /// Even if IANA reuses this code, this library will preserve the teapot’s legacy.
    /// My favorite, I'd be very happy to see you perform it 🫖 ❤️
    ImaTeapot = (418, "I'm a teapot");
    /// [[RFC9110, Section 15.5.20](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.20)]
    MisdirectedRequest = (421, "Misdirected Request");
    /// [[RFC9110, Section 15.5.21](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.21)]
    UnprocessableEntity = (422, "Unprocessable Entity");
    /// [[RFC4918, Section 11.3](https://datatracker.ietf.org/doc/html/rfc4918#section-11.3)]
    Locked = (423, "Locked");
    /// [[RFC4918, Section 11.4](https://tools.ietf.org/html/rfc4918#section-11.4)]
    FailedDependency = (424, "Failed Dependency");
    /// [[RFC8470, Section 5.2](https://httpwg.org/specs/rfc8470.html#status)]
    TooEarly = (425, "Too Early");
    /// [[RFC9110, Section 15.5.22](https://datatracker.ietf.org/doc/html/rfc9110#section-15.5.22)]
    UpgradeRequired = (426, "Upgrade Required");
    /// [[RFC6585, Section 3](https://datatracker.ietf.org/doc/html/rfc6585#section-3)]
    PreconditionRequired = (428, "Precondition Required");
    /// [[RFC6585, Section 4](https://datatracker.ietf.org/doc/html/rfc6585#section-4)]
    TooManyRequests = (429, "Too Many Requests");
    /// [[RFC6585, Section 5](https://datatracker.ietf.org/doc/html/rfc6585#section-5)]
    RequestHeaderFieldsTooLarge = (431, "Request Header Fields Too Large");
    /// [[RFC7725, Section 3](https://tools.ietf.org/html/rfc7725#section-3)]
    UnavailableForLegalReasons = (451, "Unavailable For Legal Reasons");

    /// [[RFC9110, Section 15.6.1](https://datatracker.ietf.org/doc/html/rfc9110#section-15.6.1)]
    InternalServerError = (500, "Internal Server Error");
    /// [[RFC9110, Section 15.6.2](https://datatracker.ietf.org/doc/html/rfc9110#section-15.6.2)]
    NotImplemented = (501, "Not Implemented");
    /// [[RFC9110, Section 15.6.3](https://datatracker.ietf.org/doc/html/rfc9110#section-15.6.3)]
    BadGateway = (502, "Bad Gateway");
    /// [[RFC9110, Section 15.6.4](https://datatracker.ietf.org/doc/html/rfc9110#section-15.6.4)]
    ServiceUnavailable = (503, "Service Unavailable");
    /// [[RFC9110, Section 15.6.5](https://datatracker.ietf.org/doc/html/rfc9110#section-15.6.5)]
    GatewayTimeout = (504, "Gateway Timeout");
    /// [[RFC9110, Section 15.6.6](https://datatracker.ietf.org/doc/html/rfc9110#section-15.6.6)]
    HttpVersionNotSupported = (505, "HTTP Version Not Supported");
    /// [[RFC2295, Section 8.1](https://datatracker.ietf.org/doc/html/rfc2295#section-8.1)]
    VariantAlsoNegotiates = (506, "Variant Also Negotiates");
    /// [[RFC4918, Section 11.5](https://datatracker.ietf.org/doc/html/rfc4918#section-11.5)]
    InsufficientStorage = (507, "Insufficient Storage");
    /// [[RFC5842, Section 7.2](https://datatracker.ietf.org/doc/html/rfc5842#section-7.2)]
    LoopDetected = (508, "Loop Detected");
    /// [[RFC2774, Section 7](https://datatracker.ietf.org/doc/html/rfc2774#section-7)]
    NotExtended = (510, "Not Extended");
    /// [[RFC6585, Section 6](https://datatracker.ietf.org/doc/html/rfc6585#section-6)]
    NetworkAuthenticationRequired = (511, "Network Authentication Required");
}

// Url

/// A parsed URL representation optimized for HTTP request handling.
///
/// # Components
///
/// - **Target**: Full path with query string (e.g., `/api/users/123?sort=name&debug`)
/// - **Path**: Path without query string (e.g., `/api/users/123`)  
/// - **Segments**: Path split by `/` (e.g., `["api", "users", "123"]`)
/// - **Query**: Optional query string with parameters
///
/// # Note
/// In HTTP/0.9+, the `/keep_alive` prefix is removed if present (applies to all methods).
///
/// Example:
/// ```
/// let url = "/keep_alive/api/users";
///
/// // Parsing...
///
/// // HTTP/1.x
/// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
/// assert_eq!(req.url().path_str(), "/keep_alive/api/users");
/// # });
/// #
/// // HTTP/0.9+
/// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
/// assert_eq!(req.url().path_str(), "/api/users");
/// # });
/// ```
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Url {
    pub(crate) target: &'static str,
    pub(crate) path: &'static str,
    pub(crate) parts: Vec<&'static str>,
    pub(crate) query: Option<&'static str>,
    // If you make &str, you'll either have to use `std::str::from_utf8`, which
    // will hurt performance, or `std::str::from_utf8_unchecked`, which requires
    // valid data (the public API can't provide it).
    pub(crate) query_parts: Vec<(&'static [u8], &'static [u8])>,
    // For HTTP/0.9+ (ignoring prefix `/keep_alive`)
    pub(crate) skip_first_segment: bool,
}

impl Url {
    #[inline]
    pub(crate) fn new(limits: &ReqLimits) -> Self {
        Self {
            target: "",
            path: "",
            parts: Vec::with_capacity(limits.url_parts),
            query: None,
            query_parts: Vec::with_capacity(limits.url_query_parts),
            skip_first_segment: false,
        }
    }

    #[inline]
    pub(crate) fn clear(&mut self) {
        self.target = "";
        self.path = "";
        self.parts.clear();
        self.query = None;
        self.query_parts.clear();
        self.skip_first_segment = false;
    }
}

/// Methods for working with URL as slice string
impl Url {
    /// Returns the raw request target as string slice.
    ///
    /// # Examples
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert_eq!(req.url().target_str(), "/api/users/123?sort=name&debug");
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert_eq!(req.url().target_str(), "/api/users/123?sort=name&debug");
    /// # });
    /// ```
    #[inline(always)]
    pub fn target_str(&self) -> &str {
        &self.target[11 * self.skip_first_segment as usize..]
    }

    /// Returns the path component of the URL without the query string as bytes.
    ///
    /// # Examples
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert_eq!(req.url().path_str(), "/api/users/123");
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert_eq!(req.url().path_str(), "/api/users/123");
    /// # });
    /// ```
    #[inline(always)]
    pub fn path_str(&self) -> &str {
        &self.path[11 * self.skip_first_segment as usize..]
    }

    /// Returns the path segment at the specified index.
    ///
    /// Path segments are the parts between `/` characters.
    /// Index 0 is the first segment after the initial `/`.
    ///
    /// # Examples
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert_eq!(req.url().path_segment_str(0), Some("api"));
    /// assert_eq!(req.url().path_segment_str(1), Some("users"));
    /// assert_eq!(req.url().path_segment_str(2), Some("123"));
    /// assert_eq!(req.url().path_segment_str(3), None);
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert_eq!(req.url().path_segment_str(0), Some("api"));
    /// # assert_eq!(req.url().path_segment_str(1), Some("users"));
    /// # assert_eq!(req.url().path_segment_str(2), Some("123"));
    /// # assert_eq!(req.url().path_segment_str(3), None);
    /// # });
    /// ```
    #[inline(always)]
    pub fn path_segment_str(&self, index: usize) -> Option<&str> {
        self.parts
            .get(index + self.skip_first_segment as usize)
            .copied()
    }

    /// Returns all path segments as slice
    ///
    /// Segments are split by `/` characters and do not include the leading or
    /// trailing slashes.
    ///
    /// # Examples
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert_eq!(req.url().path_segments_str(), ["api", "users", "123"]);
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert_eq!(req.url().path_segments_str(), ["api", "users", "123"]);
    /// # });
    /// ```
    #[inline(always)]
    pub fn path_segments_str(&self) -> &[&str] {
        &self.parts[self.skip_first_segment as usize..]
    }

    /// Checks if the path matches the given pattern.
    ///
    /// # Examples
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert!(!req.url().matches_str(&["api"]));
    /// assert!(req.url().matches_str(&["api", "users", "123"]));
    /// assert!(!req.url().matches_str(&["api", "users"]));
    /// assert!(!req.url().matches_str(&["api", "users", "123", "name"]));
    /// assert!(!req.url().matches_str(&["users", "123"]));
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert!(!req.url().matches_str(&["api"]));
    /// # assert!(req.url().matches_str(&["api", "users", "123"]));
    /// # assert!(!req.url().matches_str(&["api", "users"]));
    /// # assert!(!req.url().matches_str(&["api", "users", "123", "name"]));
    /// # assert!(!req.url().matches_str(&["users", "123"]));
    /// # });
    /// ```
    #[inline(always)]
    pub fn matches_str(&self, pattern: &[&str]) -> bool {
        self.path_segments_str() == pattern
    }

    /// Checks if the path starts with the given pattern.
    ///
    /// Useful for route prefix matching.
    ///
    /// # Examples
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert!(req.url().starts_with_str(&["api"]));
    /// assert!(req.url().starts_with_str(&["api", "users", "123"]));
    /// assert!(req.url().starts_with_str(&["api", "users"]));
    /// assert!(!req.url().starts_with_str(&["api", "users", "123", "name"]));
    /// assert!(!req.url().starts_with_str(&["users", "123"]));
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert!(req.url().starts_with_str(&["api"]));
    /// # assert!(req.url().starts_with_str(&["api", "users", "123"]));
    /// # assert!(req.url().starts_with_str(&["api", "users"]));
    /// # assert!(!req.url().starts_with_str(&["api", "users", "123", "name"]));
    /// # assert!(!req.url().starts_with_str(&["users", "123"]));
    /// # });
    /// ```
    #[inline(always)]
    pub fn starts_with_str(&self, pattern: &[&str]) -> bool {
        self.path_segments_str().starts_with(pattern)
    }

    /// Checks if the path ends with the given pattern.
    ///
    /// Useful for file extension matching.
    ///
    /// # Examples
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert!(!req.url().ends_with_str(&["api"]));
    /// assert!(req.url().ends_with_str(&["api", "users", "123"]));
    /// assert!(!req.url().ends_with_str(&["api", "users"]));
    /// assert!(!req.url().ends_with_str(&["api", "users", "123", "name"]));
    /// assert!(req.url().ends_with_str(&["users", "123"]));
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert!(!req.url().ends_with_str(&["api"]));
    /// # assert!(req.url().ends_with_str(&["api", "users", "123"]));
    /// # assert!(!req.url().ends_with_str(&["api", "users"]));
    /// # assert!(!req.url().ends_with_str(&["api", "users", "123", "name"]));
    /// # assert!(req.url().ends_with_str(&["users", "123"]));
    /// # });
    /// ```
    #[inline(always)]
    pub fn ends_with_str(&self, pattern: &[&str]) -> bool {
        self.path_segments_str().ends_with(pattern)
    }

    /// Returns the full query string including the leading `?`.
    ///
    /// Returns `None` if no query string is present.
    ///
    /// # Examples
    /// Request without query:
    /// ```
    /// let url = "/api/users/123";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert_eq!(req.url().query_full_str(), None);
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert_eq!(req.url().query_full_str(), None);
    /// # });
    /// ```
    /// Request with query:
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert_eq!(req.url().query_full_str(), Some("?sort=name&debug"));
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert_eq!(req.url().query_full_str(), Some("?sort=name&debug"));
    /// # });
    /// ```
    #[inline(always)]
    pub fn query_full_str(&self) -> Option<&str> {
        self.query
    }

    /// Returns the value for the specified query parameter key.
    ///
    /// Performs case-sensitive lookup. Returns the first value
    /// if multiple parameters with the same key exist.
    ///
    /// # Examples
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert_eq!(req.url().query_str("sort"), Some("name"));
    /// assert_eq!(req.url().query_str("debug"), Some(""));
    /// assert_eq!(req.url().query_str("name"), None);
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert_eq!(req.url().query_str("sort"), Some("name"));
    /// # assert_eq!(req.url().query_str("debug"), Some(""));
    /// # assert_eq!(req.url().query_str("name"), None);
    /// # });
    /// ```
    #[inline(always)]
    pub fn query_str(&self, key: &str) -> Option<&str> {
        self.query_parts
            .iter()
            .find(|&&(k, _)| k == key.as_bytes())
            .map(|&(_, v)| unsafe {
                // SAFETY: This method is only available after the request
                // (except the body) has been validated with `simdutf8`, which
                // ensures that the data is `UTF-8`.
                std::str::from_utf8_unchecked(v)
            })
    }
}

/// Methods for working with URL as bytes
impl Url {
    /// Returns the raw request target
    ///
    /// # Examples
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert_eq!(req.url().target(), b"/api/users/123?sort=name&debug");
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert_eq!(req.url().target(), b"/api/users/123?sort=name&debug");
    /// # });
    /// ```
    #[inline(always)]
    pub fn target(&self) -> &[u8] {
        self.target_str().as_bytes()
    }

    /// Returns the path component of the URL without the query string
    ///
    /// # Examples
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert_eq!(req.url().path(), b"/api/users/123");
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert_eq!(req.url().path(), b"/api/users/123");
    /// # });
    /// ```
    #[inline(always)]
    pub fn path(&self) -> &[u8] {
        self.path_str().as_bytes()
    }

    /// Returns the path segment at the specified index
    ///
    /// Path segments are the parts between `/` characters.
    /// Index 0 is the first segment after the initial `/`.
    ///
    /// # Examples
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert_eq!(req.url().path_segment(0), Some("api".as_bytes()));
    /// assert_eq!(req.url().path_segment(1), Some("users".as_bytes()));
    /// assert_eq!(req.url().path_segment(2), Some("123".as_bytes()));
    /// assert_eq!(req.url().path_segment(3), None);
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert_eq!(req.url().path_segment(0), Some("api".as_bytes()));
    /// # assert_eq!(req.url().path_segment(1), Some("users".as_bytes()));
    /// # assert_eq!(req.url().path_segment(2), Some("123".as_bytes()));
    /// # assert_eq!(req.url().path_segment(3), None);
    /// # });
    /// ```
    #[inline(always)]
    pub fn path_segment(&self, index: usize) -> Option<&[u8]> {
        self.path_segment_str(index).map(|v| v.as_bytes())
    }

    /// Returns all path segments as a slice.
    ///
    /// Segments are split by `/` characters and do not include the leading or
    /// trailing slashes.
    ///
    /// # Examples
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert_eq!(
    ///     req.url().path_segments(),
    ///     ["api".as_bytes(), "users".as_bytes(), "123".as_bytes()]
    /// );
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert_eq!(
    /// #     req.url().path_segments(),
    /// #     ["api".as_bytes(), "users".as_bytes(), "123".as_bytes()]
    /// # );
    /// # });
    /// ```
    #[inline(always)]
    pub fn path_segments(&self) -> &[&[u8]] {
        const _: () = assert!(mem::size_of::<&str>() == mem::size_of::<&[u8]>());
        const _: () = assert!(mem::align_of::<&str>() == mem::align_of::<&[u8]>());

        const _: () = assert!(mem::size_of::<&[&str]>() == mem::size_of::<&[&[u8]]>());
        const _: () = assert!(mem::align_of::<&[&str]>() == mem::align_of::<&[&[u8]]>());

        let str_slice: &[&str] = &self.parts[self.skip_first_segment as usize..];

        // SAFETY: Safe because &str and &[u8] have identical memory layout
        // (both are fat pointers: data pointer + length). All strings are
        // guaranteed to be valid UTF-8 from parsing.
        unsafe { mem::transmute::<&[&str], &[&[u8]]>(str_slice) }
    }

    /// Checks if the path matches the given pattern.
    ///
    /// # Examples
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert!(!req.url().matches(&[b"api"]));
    /// assert!(req.url().matches(&[b"api", b"users", b"123"]));
    /// assert!(!req.url().matches(&[b"api", b"users"]));
    /// assert!(!req.url().matches(&[b"api", b"users", b"123", b"name"]));
    /// assert!(!req.url().matches(&[b"users", b"123"]));
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert!(!req.url().matches(&[b"api"]));
    /// # assert!(req.url().matches(&[b"api", b"users", b"123"]));
    /// # assert!(!req.url().matches(&[b"api", b"users"]));
    /// # assert!(!req.url().matches(&[b"api", b"users", b"123", b"name"]));
    /// # assert!(!req.url().matches(&[b"users", b"123"]));
    /// # });
    /// ```
    #[inline(always)]
    pub fn matches(&self, pattern: &[&[u8]]) -> bool {
        self.path_segments() == pattern
    }

    /// Checks if the path starts with the given pattern.
    ///
    /// Useful for route prefix matching.
    ///
    /// # Examples
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert!(req.url().starts_with(&[b"api"]));
    /// assert!(req.url().starts_with(&[b"api", b"users", b"123"]));
    /// assert!(req.url().starts_with(&[b"api", b"users"]));
    /// assert!(!req.url().starts_with(&[b"api", b"users", b"123", b"name"]));
    /// assert!(!req.url().starts_with(&[b"users", b"123"]));
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert!(req.url().starts_with(&[b"api"]));
    /// # assert!(req.url().starts_with(&[b"api", b"users", b"123"]));
    /// # assert!(req.url().starts_with(&[b"api", b"users"]));
    /// # assert!(!req.url().starts_with(&[b"api", b"users", b"123", b"name"]));
    /// # assert!(!req.url().starts_with(&[b"users", b"123"]));
    /// # });
    /// ```
    #[inline(always)]
    pub fn starts_with(&self, pattern: &[&[u8]]) -> bool {
        self.path_segments().starts_with(pattern)
    }

    /// Checks if the path ends with the given pattern.
    ///
    /// Useful for file extension matching.
    ///
    /// # Examples
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert!(!req.url().ends_with(&[b"api"]));
    /// assert!(req.url().ends_with(&[b"api", b"users", b"123"]));
    /// assert!(!req.url().ends_with(&[b"api", b"users"]));
    /// assert!(!req.url().ends_with(&[b"api", b"users", b"123", b"name"]));
    /// assert!(req.url().ends_with(&[b"users", b"123"]));
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert!(!req.url().ends_with(&[b"api"]));
    /// # assert!(req.url().ends_with(&[b"api", b"users", b"123"]));
    /// # assert!(!req.url().ends_with(&[b"api", b"users"]));
    /// # assert!(!req.url().ends_with(&[b"api", b"users", b"123", b"name"]));
    /// # assert!(req.url().ends_with(&[b"users", b"123"]));
    /// # });
    /// ```
    #[inline(always)]
    pub fn ends_with(&self, pattern: &[&[u8]]) -> bool {
        self.path_segments().ends_with(pattern)
    }

    /// Returns the full query string including the leading `?`.
    ///
    /// Returns `None` if no query string is present.
    ///
    /// # Examples
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert_eq!(req.url().query_full(), Some("?sort=name&debug".as_bytes()));
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert_eq!(req.url().query_full(), Some("?sort=name&debug".as_bytes()));
    /// # });
    /// ```
    #[inline(always)]
    pub fn query_full(&self) -> Option<&[u8]> {
        self.query.map(|value| value.as_bytes())
    }

    /// Returns the value for the specified query parameter key.
    ///
    /// Performs case-sensitive lookup. Returns the first value
    /// if multiple parameters with the same key exist.
    ///
    /// # Examples
    /// ```
    /// let url = "/api/users/123?sort=name&debug";
    ///
    /// // Parsing...
    ///
    /// # maker_web::docs_rs_helper::example_url_http1x(url, |req| {
    /// assert_eq!(req.url().query(b"sort"), Some("name".as_bytes()));
    /// assert_eq!(req.url().query(b"debug"), Some("".as_bytes()));
    /// assert_eq!(req.url().query(b"name"), None);
    /// # });
    /// #
    /// # maker_web::docs_rs_helper::example_url_http09(url, |req| {
    /// # assert_eq!(req.url().query(b"sort"), Some("name".as_bytes()));
    /// # assert_eq!(req.url().query(b"debug"), Some("".as_bytes()));
    /// # assert_eq!(req.url().query(b"name"), None);
    /// # });
    /// ```
    #[inline(always)]
    pub fn query(&self, key: &[u8]) -> Option<&[u8]> {
        self.query_parts
            .iter()
            .find(|&&(k, _)| k == key)
            .map(|&(_, v)| v)
    }
}

// HEADER

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub(crate) struct Header {
    pub(crate) name: &'static str,
    pub(crate) value: &'static str,
}
