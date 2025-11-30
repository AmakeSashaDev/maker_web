#![allow(rustdoc::bare_urls)]

//! Core HTTP protocol types and utilities

use crate::{errors::ErrorKind, limits::ReqLimits};

// TO LOWER CASE

#[rustfmt::skip]
const ASCII_TABLE: [u8; 256] = [
    //   x0    x1    x2    x3    x4    x5    x6    x7    x8    x9    xA    xB    xC    xD    xE    xF
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, // 0x
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F, // 1x
    0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F, // 2x
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F, // 3x
    0x40, b'a', b'b', b'c', b'd', b'e', b'f', b'g', b'h', b'i', b'j', b'k', b'l', b'm', b'n', b'o', // 4x
    b'p', b'q', b'r', b's', b't', b'u', b'v', b'w', b'x', b'y', b'z', 0x5B, 0x5C, 0x5D, 0x5E, 0x5F, // 5x
    0x60, b'a', b'b', b'c', b'd', b'e', b'f', b'g', b'h', b'i', b'j', b'k', b'l', b'm', b'n', b'o', // 6x
    b'p', b'q', b'r', b's', b't', b'u', b'v', b'w', b'x', b'y', b'z', 0x7B, 0x7C, 0x7D, 0x7E, 0x7F, // 7x
    0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x8B, 0x8C, 0x8D, 0x8E, 0x8F, // 8x
    0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0x9B, 0x9C, 0x9D, 0x9E, 0x9F, // 9x
    0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xAB, 0xAC, 0xAD, 0xAE, 0xAF, // Ax
    0xB0, 0xB1, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xBB, 0xBC, 0xBD, 0xBE, 0xBF, // Bx
    0xC0, 0xC1, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xCB, 0xCC, 0xCD, 0xCE, 0xCF, // Cx
    0xD0, 0xD1, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xDB, 0xDC, 0xDD, 0xDE, 0xDF, // Dx
    0xE0, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xEB, 0xEC, 0xED, 0xEE, 0xEF, // Ex
    0xF0, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFB, 0xFC, 0xFD, 0xFE, 0xFF, // Fx
];

#[inline(always)]
pub(crate) fn to_lower_case(src: &mut [u8]) {
    for byte in src.iter_mut() {
        *byte = ASCII_TABLE[*byte as usize];
    }
}

#[inline(always)]
pub(crate) fn into_lower_case(src: &[u8], result: &mut [u8]) -> usize {
    let len = src.len().min(result.len());
    for i in 0..len {
        result[i] = ASCII_TABLE[src[i] as usize];
    }
    len
}

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
    /// GET method - transfer a current representation of the target resource
    /// [[RFC7231, Section 4.3.1](https://tools.ietf.org/html/rfc7231#section-4.3.1)]
    Get,
    /// PUT method - replace all current representations of the target resource with the request payload
    /// [[RFC7231, Section 4.3.4](https://tools.ietf.org/html/rfc7231#section-4.3.4)]
    Put,
    /// POST method - perform resource-specific processing on the request payload
    /// [[RFC7231, Section 4.3.3](https://tools.ietf.org/html/rfc7231#section-4.3.3)]
    Post,
    /// HEAD method - same as GET but without response body
    /// [[RFC7231, Section 4.3.2](https://tools.ietf.org/html/rfc7231#section-4.3.2)]
    Head,
    /// PATCH method - apply partial modifications to a resource
    /// [[RFC5789, Section 2](https://tools.ietf.org/html/rfc5789#section-2)]
    Patch,
    /// DELETE method - remove all current representations of the target resource
    /// [[RFC7231, Section 4.3.5](https://tools.ietf.org/html/rfc7231#section-4.3.5)]
    Delete,
    /// OPTIONS method - describe the communication options for the target resource
    /// [[RFC7231, Section 4.3.7](https://tools.ietf.org/html/rfc7231#section-4.3.7)]
    Options,
}

impl Method {
    #[inline(always)]
    pub(crate) fn from_bytes(src: &[u8]) -> Result<(Self, usize), ErrorKind> {
        match src {
            [b'G', b'E', b'T', b' ', ..] => Ok((Method::Get, 4)),
            [b'P', b'U', b'T', b' ', ..] => Ok((Method::Put, 4)),
            [b'P', b'O', b'S', b'T', b' ', ..] => Ok((Method::Post, 5)),
            [b'H', b'E', b'A', b'D', b' ', ..] => Ok((Method::Head, 5)),
            [b'P', b'A', b'T', b'C', b'H', b' ', ..] => Ok((Method::Patch, 6)),
            [b'D', b'E', b'L', b'E', b'T', b'E', b' ', ..] => Ok((Method::Delete, 7)),
            [b'O', b'P', b'T', b'I', b'O', b'N', b'S', b' ', ..] => Ok((Method::Options, 8)),
            _ => Err(ErrorKind::InvalidMethod),
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
    #[inline(always)]
    pub(crate) const fn from_bytes(src: &[u8]) -> Result<(Self, bool), ErrorKind> {
        match src {
            b"HTTP/1.1" => Ok((Self::Http11, true)),
            b"HTTP/1.0" => Ok((Self::Http10, false)),
            _ => Err(ErrorKind::UnsupportedVersion),
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
            pub(crate) const fn into_first_line(&self, version: Version) -> &'static [u8] {
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
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Url {
    pub(crate) target: &'static [u8],
    pub(crate) path: &'static [u8],
    pub(crate) parts: Vec<&'static [u8]>,
    pub(crate) query: Option<&'static [u8]>,
    pub(crate) query_parts: Vec<(&'static [u8], &'static [u8])>,
}

impl Url {
    #[inline(always)]
    pub(crate) fn new(limits: &ReqLimits) -> Self {
        Self {
            target: b"",
            path: b"",
            parts: Vec::with_capacity(limits.url_parts),
            query: None,
            query_parts: Vec::with_capacity(limits.url_query_parts),
        }
    }

    #[inline(always)]
    pub(crate) fn clear(&mut self) {
        self.target = b"";
        self.path = b"";
        self.parts.clear();
        self.query = None;
        self.query_parts.clear();
    }
}

// Public API
impl Url {
    /// Returns the raw request target as bytes.
    ///
    /// The target is the full path and query string from the request line.
    ///
    /// # Examples
    ///
    /// For path `/api/users/123?sort=name&debug`:
    /// ```text
    /// /api/users/123?sort=name&debug
    /// ```
    #[inline(always)]
    pub const fn target(&self) -> &[u8] {
        self.target
    }

    /// Returns the path component of the URL.
    ///
    /// This is the target without the query string.
    ///
    /// # Examples
    ///
    /// For path `/api/users/123?sort=name&debug`:
    /// ```text
    /// /api/users/123
    /// ```
    #[inline(always)]
    pub const fn path(&self) -> &[u8] {
        self.path
    }

    /// Returns the path segment at the specified index.
    ///
    /// Path segments are the parts between `/` characters.
    /// Index 0 is the first segment after the initial `/`.
    ///
    /// # Examples
    ///
    /// For path `/api/users/123?sort=name&debug`:
    /// - index `0`: `Some(b"api")`
    /// - index `1`: `Some(b"users")`
    /// - index `2`: `Some(b"123")`
    /// - index `3`: `None`
    #[inline(always)]
    pub fn path_segment(&self, index: usize) -> Option<&[u8]> {
        self.parts.get(index).copied()
    }

    /// Returns all path segments as a slice.
    ///
    /// Segments are split by `/` characters and do not include
    /// the leading or trailing slashes.
    ///
    /// # Examples
    ///
    /// For path `/api/users/123?sort=name&debug`:
    /// ```text
    /// [b"api", b"users", b"123"]
    /// ```
    #[inline(always)]
    pub fn path_segments(&self) -> &[&[u8]] {
        self.parts.as_slice()
    }

    /// Checks if the path matches the given pattern.
    ///
    /// The pattern should be an array of byte slices representing
    /// the expected path segments.
    ///
    /// # Examples
    ///
    /// For path `/api/users/123?sort=name&debug`:
    /// - pattern `&[b"api"]`: `false`
    /// - pattern `&[b"api", b"users", b"123"]`: `true`
    /// - pattern `&[b"api", b"users"]`: `false`
    /// - pattern `&[b"api", b"users", b"123", b"name"]`: `false`
    /// - pattern `&[b"users", b"123"]`: `false`
    #[inline(always)]
    pub fn matches(&self, pattern: &[&[u8]]) -> bool {
        self.path_segments() == pattern
    }

    /// Checks if the path starts with the given pattern.
    ///
    /// Useful for route prefix matching.
    ///
    /// # Examples
    ///
    /// For path `/api/users/123?sort=name&debug`:
    /// - pattern `&[b"api"]`: `true`
    /// - pattern `&[b"api", b"users", b"123"]`: `true`
    /// - pattern `&[b"api", b"users"]`: `true`
    /// - pattern `&[b"api", b"users", b"123", b"name"]`: `false`
    /// - pattern `&[b"users", b"123"]`: `false`
    #[inline(always)]
    pub fn starts_with(&self, pattern: &[&[u8]]) -> bool {
        self.path_segments().starts_with(pattern)
    }

    /// Checks if the path ends with the given pattern.
    ///
    /// Useful for file extension matching.
    ///
    /// # Examples
    ///
    /// For path `/api/users/123?sort=name&debug`:
    /// - pattern `&[b"api"]`: `false`
    /// - pattern `&[b"api", b"users", b"123"]`: `true`
    /// - pattern `&[b"api", b"users"]`: `false`
    /// - pattern `&[b"api", b"users", b"123", b"name"]`: `false`
    /// - pattern `&[b"users", b"123"]`: `true`
    #[inline(always)]
    pub fn ends_with(&self, pattern: &[&[u8]]) -> bool {
        self.path_segments().ends_with(pattern)
    }

    /// Returns the full query string including the leading `?`.
    ///
    /// Returns `None` if no query string is present.
    ///
    /// # Examples
    ///
    /// For path `/api/users/123?sort=name&debug`:
    /// ```text
    /// ?sort=name&debug
    /// ```
    #[inline(always)]
    pub const fn query_full(&self) -> Option<&[u8]> {
        self.query
    }

    /// Returns the value for the specified query parameter key.
    ///
    /// Performs case-sensitive lookup. Returns the first value
    /// if multiple parameters with the same key exist.
    ///
    /// # Arguments
    ///
    /// - `key`: Parameter name to look up (e.g., `b"sort"`)
    ///
    /// # Examples
    ///
    /// For path `/api/users/123?sort=name&debug`:
    /// - at the key `b"sort"`: `Some(b"name")`
    /// - at the key `b"debug"`: `Some(b"")`
    /// - at the key `b"something"`: `None`
    #[inline(always)]
    pub fn query(&self, key: &[u8]) -> Option<&[u8]> {
        self.query_parts
            .iter()
            .find(|&&(k, _)| k == key)
            .map(|&(_, v)| v)
    }
}

// HEADER MAP

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct HeaderMap {
    pub(crate) headers: Vec<Header>,
    pub(crate) content_length: Option<usize>,
}

impl HeaderMap {
    #[inline(always)]
    pub(crate) fn new(size_vec: usize) -> Self {
        Self {
            headers: Vec::with_capacity(size_vec),
            content_length: None,
        }
    }

    #[inline(always)]
    pub(crate) fn reset(&mut self) {
        self.headers.clear();
        self.content_length = None;
    }

    #[inline(always)]
    pub(crate) fn get(&self, name: &[u8]) -> Option<&[u8]> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value)
    }
}

// HEADER

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub(crate) struct Header {
    pub(crate) name: &'static [u8],
    pub(crate) value: &'static [u8],
}

impl Header {
    #[inline(always)]
    pub const fn new(name: &'static [u8], value: &'static [u8]) -> Self {
        Header { name, value }
    }
}
