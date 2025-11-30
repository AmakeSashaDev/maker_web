use crate::{query, Version};
use std::{error, fmt, io};

#[derive(Debug, PartialEq)]
pub(crate) enum ErrorKind {
    InvalidMethod,

    InvalidUrl,
    #[allow(dead_code)]
    Query(query::Error),

    InvalidVersion,
    UnsupportedVersion,

    InvalidHeader,
    TooManyHeaders,
    InvalidContentLength,
    InvalidConnection,

    BodyTooLarge,
    #[allow(dead_code)]
    BodyMismatch {
        expected: usize,
        available: usize,
    },
    #[allow(dead_code)]
    UnexpectedBody(usize),

    ServiceUnavailable,
    Io(IoError),
}

macro_rules! http_errors {
    ($($name:ident: $status_code:expr, $len:literal => $json:literal; )*) => {
        pub(crate) const fn as_http(
            &self,
            version: Version,
            json: bool,
        ) -> &'static [u8] {
            match (json, self, version) { $(
                (true, Self::$name { .. }, Version::Http11) => concat!(
                    "HTTP/1.1 ", $status_code, "\r\n",
                    "connection: close\r\n",
                    "content-length: ", $len, "\r\n",
                    "content-type: application/json\r\n",
                    "\r\n",
                    $json
                ),
                (false, Self::$name { .. }, Version::Http11) => concat!(
                    "HTTP/1.1 ", $status_code, "\r\n",
                    "connection: close\r\n",
                    "content-length: 0\r\n\r\n",
                ),
                (true, Self::$name { .. }, Version::Http10) => concat!(
                    "HTTP/1.0 ", $status_code, "\r\n",
                    "connection: close\r\n",
                    "content-length: ", $len, "\r\n",
                    "content-type: application/json\r\n",
                    "\r\n",
                    $json
                ),
                (false, Self::$name { .. }, Version::Http10) => concat!(
                    "HTTP/1.0 ", $status_code, "\r\n",
                    "connection: close\r\n",
                    "content-length: 0\r\n\r\n",
                ),
                (_, Self::$name { .. }, Version::Http09) => concat!(
                    "ERROR: ", stringify!($status_code)
                ),
            )* }.as_bytes()
        }
    };
}

impl ErrorKind {
    http_errors! {
        InvalidMethod: "400 Bad Request", "55"
            => r#"{"error":"Invalid HTTP method","code":"INVALID_METHOD"}"#;

        InvalidUrl: "400 Bad Request", "51"
            => r#"{"error":"Invalid URL format","code":"INVALID_URL"}"#;
        Query: "400 Bad Request", "55"
            => r#"{"error":"Invalid query string","code":"INVALID_QUERY"}"#;

        InvalidVersion: "400 Bad Request", "57"
            => r#"{"error":"Invalid HTTP version","code":"INVALID_VERSION"}"#;
        UnsupportedVersion: "505 HTTP Version Not Supported", "67"
            => r#"{"error":"HTTP version not supported","code":"UNSUPPORTED_VERSION"}"#;

        InvalidHeader: "400 Bad Request", "57"
            => r#"{"error":"Invalid header format","code":"INVALID_HEADER"}"#;
        TooManyHeaders: "431 Request Header Fields Too Large", "54"
            => r#"{"error":"Too many headers","code":"TOO_MANY_HEADERS"}"#;
        InvalidContentLength: "400 Bad Request", "66"
            => r#"{"error":"Invalid Content-Length","code":"INVALID_CONTENT_LENGTH"}"#;
        InvalidConnection: "400 Bad Request", "65"
            => r#"{"error":"Invalid Connection header","code":"INVALID_CONNECTION"}"#;

        BodyTooLarge: "413 Payload Too Large", "58"
            => r#"{"error":"Request body too large","code":"BODY_TOO_LARGE"}"#;
        BodyMismatch: "400 Bad Request", "55"
            => r#"{"error":"Body length mismatch","code":"BODY_MISMATCH"}"#;
        UnexpectedBody: "400 Bad Request", "60"
            => r#"{"error":"Unexpected request body","code":"UNEXPECTED_BODY"}"#;

        ServiceUnavailable: "503 Service Unavailable", "72"
            => r#"{"error":"Service temporarily unavailable","code":"SERVICE_UNAVAILABLE"}"#;
        Io: "503 Service Unavailable", "48"
            => r#"{"error":"I/O error occurred","code":"IO_ERROR"}"#;
    }
}

impl error::Error for ErrorKind {}
impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<query::Error> for ErrorKind {
    fn from(err: query::Error) -> Self {
        ErrorKind::Query(err)
    }
}
impl From<io::Error> for ErrorKind {
    fn from(err: io::Error) -> Self {
        ErrorKind::Io(IoError(err))
    }
}

#[derive(Debug)]
pub(crate) struct IoError(pub(crate) io::Error);

impl PartialEq for IoError {
    fn eq(&self, other: &Self) -> bool {
        self.0.kind() == other.0.kind()
    }
}
