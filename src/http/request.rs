use crate::{
    errors::*,
    http::types::{self, Header},
    limits::ReqLimits,
    query::Query,
    server::connection::HttpConnection,
    ConnectionData, Handler, Method, Url, Version,
};
use memchr::{memchr2_iter, memchr3_iter, Memchr3};
use std::{
    io, mem,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    str,
    time::Duration,
};
use tokio::{io::AsyncReadExt, net::TcpStream, time::sleep};

/// High-performance HTTP request representation.
///
/// Uses strategic memory alignment for optimal cache performance.
/// All data is zero-copy referenced from the original input.
///
/// # Input data requirements
///
/// All incoming data must match the specified template string. Failure to do so
/// may result in parsing errors (don't panic) or incorrect parsing. See each
/// section for details.
///
/// #### Character encoding
///
/// The entire request, except the body, must be in `UTF-8`. Otherwise, the parser
/// will consider the request invalid. This is done to ensure safe handling of `&str`.
/// If you need to transfer binary data, you can use `base64` and similar encodings.
///
/// #### General designations
/// - `SP`: ASCII space (0x20)
/// - `CRLF`: Carriage return + line feed (`"\r\n"`) - **exactly this sequence required**
///
///   Unlike RFC 7230 which permits `CR`, `LF`, or `CRLF` in certain contexts,
///   this parser requires exactly `CRLF` as line terminator.
/// ---
/// - `[METHOD]`: See the values in structure [Method](crate::Method)
/// - `[PATH]`: URI path component, see
///   [[RFC3986, Section 3.3](https://datatracker.ietf.org/doc/html/rfc3986#section-3.3)].
///   May be followed by query component, see
///   [[RFC3986, Section 3.4](https://datatracker.ietf.org/doc/html/rfc3986#section-3.4)].
///
///   **Contrary to the RFC**: When two or more consecutive slashes (`/`)
///   are encountered, the parser  registers this as a request error.
///   The server returns `400 Bad Request` with the error message.
///
///   Example:
///   ```text
///   /api/users/123     # Ok
///   /api/users/123/    # Ok
///   //api/users/123    # Error
///   /api//users/123    # Error
///   /api/users/123//   # Error
///   ```
///
/// ## First line
/// | Version    | Template                                       | Example                       |
/// |------------|------------------------------------------------|-------------------------------|
/// | `HTTP/1.x` | `[METHOD] SP [PATH] SP "HTTP/" [VERSION] CRLF` | `GET /api/users HTTP/1.1\r\n` |
/// | `HTTP/0.9` | `[METHOD] SP [PATH] CRLF`                      | `GET /api/users\r\n`          |
///
/// **Invalid**:
/// ```text
/// GET /api/users HTTP/1.1\n   // Missing CR
/// GET /api/users HTTP/1.1\r   // Missing LF
/// ```
///
/// Where:
/// - `[VERSION]`: `1.0` or `1.1`
///
/// ## Header
///  
/// Template string:
/// ```text
/// [NAME]: SP [VALUE] CRLF
/// ```
/// Where:
/// - `[NAME]`: Header field name (ASCII, case-insensitive)
/// - `[VALUE]`: Header value (may be empty, preserves leading/trailing spaces)
///
/// Examples:
/// ```text
/// Content-Type: plain/text\r\n
/// X-Empty: \r\n                   // Empty value allowed
/// Name:   invalid value  \r\n     // Value: `  invalid value  `
/// ```
///
/// **Note**: The parser extracts semantics from two headers:
///
/// | Header           | Purpose              | Values                                                                 |
/// |------------------|----------------------|------------------------------------------------------------------------|
/// | `Content-Length` | Body size validation | Any `usize` values (not exceeding the [limits](ReqLimits::body_size))  |
/// | `Connection`     | Keep-alive flag      | `keep-alive` or `close` (case-insensitive)                             |
///
/// All other headers are preserved but not interpreted (including `Host`).
///
/// ## End of headings
///  
/// Template string:
/// ```text
/// CRLF
/// ```
///
/// ## Body
///
/// The parser only supports message bodies with an explicitly specified
/// `Content-Length` header and requires its strict compliance
///
/// **Not supported**:
/// - `Transfer-Encoding: chunked`
/// - Implicit-length bodies (read until connection close)
/// - `Expect: 100-continue`
///
/// Attempts to use unsupported methods result in error. This is an architectural
/// decision aimed at security and memory protection, and is also related to a
/// limitation of the server architecture itself. **Don't expect these features to
/// be added in the future.**
#[derive(Debug, Clone, PartialEq)]
#[repr(align(128))]
pub struct Request {
    method: Method,
    url: Url,
    version: Version,

    headers: Vec<Header>,
    content_length: Option<usize>,
    keep_alive: bool,

    body: Option<&'static [u8]>,

    pub(crate) client_addr: SocketAddr,
    pub(crate) server_addr: SocketAddr,
}

impl Request {
    const UNKNOWN_CLIENT: SocketAddr = { SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0) };
    const DEFAULT_SERVER: SocketAddr = { SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0) };

    #[inline(always)]
    pub(crate) fn new(limits: &ReqLimits) -> Self {
        Request {
            method: Method::Get,
            url: Url::new(limits),
            version: Version::Http11,

            headers: Vec::with_capacity(limits.header_count),
            content_length: None,
            keep_alive: true,

            body: None,

            client_addr: Self::UNKNOWN_CLIENT,
            server_addr: Self::DEFAULT_SERVER,
        }
    }

    #[inline(always)]
    pub(crate) fn reset(&mut self) {
        self.method = Method::Get;
        self.url.clear();
        self.version = Version::Http11;

        self.headers.clear();
        self.content_length = None;
        self.keep_alive = true;

        self.body = None;
    }
}

// Public API
impl Request {
    #[inline(always)]
    pub const fn client_addr(&self) -> &SocketAddr {
        &self.client_addr
    }

    #[inline(always)]
    pub const fn server_addr(&self) -> &SocketAddr {
        &self.server_addr
    }

    #[inline(always)]
    pub const fn method(&self) -> Method {
        self.method
    }

    #[inline(always)]
    pub const fn url(&self) -> &Url {
        &self.url
    }

    #[inline(always)]
    pub const fn version(&self) -> Version {
        self.version
    }

    /// Returns the first header value with case-insensitive name matching
    /// (per [RFC 7230](https://tools.ietf.org/html/rfc7230#section-3.2)).
    /// Uses linear search.
    #[inline(always)]
    pub fn header_str(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value)
    }

    /// Returns the first header value with case-insensitive name matching
    /// (per [RFC 7230](https://tools.ietf.org/html/rfc7230#section-3.2)).
    /// Uses linear search.
    #[inline(always)]
    pub fn header(&self, name: &[u8]) -> Option<&[u8]> {
        self.headers
            .iter()
            .find(|h| h.name.as_bytes().eq_ignore_ascii_case(name))
            .map(|h| h.value.as_bytes())
    }

    /// Returns the value of the `Content-Length` header if present.
    #[inline(always)]
    pub const fn content_length(&self) -> Option<usize> {
        self.content_length
    }

    /// Returns the keep-alive status of the connection.
    #[inline(always)]
    pub const fn is_keep_alive(&self) -> bool {
        self.keep_alive
    }

    /// Returns the request body if present.
    #[inline(always)]
    pub const fn body(&self) -> Option<&[u8]> {
        self.body
    }
}

impl<H: Handler<S>, S: ConnectionData> HttpConnection<H, S> {
    #[inline]
    pub(crate) fn parse_request(&mut self) -> Result<(), ErrorKind> {
        let mut iter = memchr2_iter(
            b'\n',
            b':',
            &self.parser.buffer[..self.req_limits.precalc.req_without_body],
        );

        let end_first_line = self
            .parser
            .find_next_crlf(&mut iter, self.req_limits.precalc.first_line)
            .filter(|i| *i <= self.req_limits.precalc.first_line)
            .ok_or(ErrorKind::InvalidVersion)?;

        // Parsing the first line
        {
            let first_line = self
                .parser
                .get_slice(0, end_first_line)
                .ok_or(ErrorKind::InvalidVersion)?;
            let mut fl_iter = memchr3_iter(b' ', b'/', b'?', first_line);

            let method_end = self.request.parse_method(&mut fl_iter, &self.parser)?;
            let space_before_version = self.request.parse_url(
                &mut fl_iter,
                [method_end, end_first_line],
                &self.parser,
                &self.req_limits,
            )?;
            let (is_end, keep_alive) = self.request.parse_version(
                &self.parser,
                space_before_version,
                end_first_line,
                self.http_09_limits.is_some(),
            )?;

            self.request.keep_alive = keep_alive;
            if is_end {
                return Ok(());
            }
        }

        // Parsing headers
        let mut start_header_line = end_first_line + 1;
        for _ in 0..=self.req_limits.header_count {
            let Some(colon) = self.parser.find_next_byte(&mut iter, b':') else {
                if self
                    .parser
                    .get_slice(start_header_line - 2, start_header_line + 2)
                    == Some(b"\r\n\r\n")
                {
                    break;
                }

                return Err(ErrorKind::InvalidHeader);
            };

            if self.request.headers.len() >= self.req_limits.header_count {
                return Err(ErrorKind::TooManyHeaders);
            }

            let crlf = iter
                .find(|pos| self.parser.buffer[*pos] == b'\n')
                .filter(|pos| self.parser.get_slice(pos - 1, pos + 1) == Some(b"\r\n"))
                .ok_or(ErrorKind::InvalidHeader)?;

            self.request.parse_header(
                &self.parser,
                &self.req_limits,
                [start_header_line, colon, crlf],
            )?;

            start_header_line = crlf + 1;
        }

        let end_headers = start_header_line + 2;

        self.parser.check_utf8(end_headers)?;

        self.request.process_body(&self.parser, end_headers)?;

        Ok(())
    }
}

// Parse first line
impl Request {
    #[inline]
    fn parse_method(&mut self, iter: &mut Memchr3, parser: &Parser) -> Result<usize, ErrorKind> {
        let method_end = parser
            .find_next_byte(iter, b' ')
            .ok_or(ErrorKind::InvalidMethod)?;

        let slice = parser
            .get_slice(0, method_end)
            .ok_or(ErrorKind::InvalidMethod)?;

        self.method = Method::from_bytes(slice)?;
        Ok(method_end)
    }

    #[inline]
    fn parse_url(
        &mut self,
        iter: &mut Memchr3,
        [method_end, end_first_line]: [usize; 2],
        parser: &Parser,
        limits: &ReqLimits,
    ) -> Result<usize, ErrorKind> {
        let start = parser
            .find_next_byte(iter, b'/')
            .ok_or(ErrorKind::InvalidUrl)?;

        if method_end + 1 != start {
            return Err(ErrorKind::InvalidUrl);
        }

        let mut end = start;
        let mut last_slash = start;
        let mut current_slash = start;
        let mut has_empty_segment = false;

        while let Some(pos) = iter.next() {
            last_slash = current_slash;
            current_slash = pos;

            Self::chekc_empty_segment(&mut has_empty_segment, pos, last_slash)?;
            self.add_url_part(parser, last_slash, current_slash)?;

            match parser.buffer[pos] {
                b'/' => {}
                b' ' => {
                    end = pos;

                    break;
                }
                b'?' => {
                    let end_query = match iter.find(|i| parser.get_byte(*i) == Some(b' ')) {
                        Some(end_query) => end_query,
                        None => {
                            if end_first_line + 1 == parser.len {
                                end_first_line - 1
                            } else {
                                return Err(ErrorKind::InvalidUrl);
                            }
                        }
                    };

                    let slice = parser
                        .get_str_static(current_slash, end_query)
                        .filter(|slice| slice.len() <= limits.url_query_size)
                        .ok_or(ErrorKind::InvalidUrl)?;

                    let limit = self.url.query_parts.capacity();
                    Query::parse_into(&mut self.url.query_parts, slice.as_bytes(), limit)?;

                    end = end_query;
                    self.url.query = Some(slice);

                    break;
                }
                _ => return Err(ErrorKind::InvalidUrl),
            }
        }

        let _ = last_slash;
        // + 1 for alignment (`end_first_line` - index, `parser.len` - length)
        match (end == start, end_first_line + 1 == parser.len) {
            (true, true) => {
                end = end_first_line - 1;

                Self::chekc_empty_segment(&mut has_empty_segment, end, current_slash)?;
                self.add_url_part(parser, current_slash, end)?;

                current_slash = end;
            }
            (true, false) => return Err(ErrorKind::InvalidUrl),
            _ => {}
        };

        self.url.target = parser
            .get_str_static(start, end)
            .filter(|target| target.len() <= limits.url_size)
            .ok_or(ErrorKind::InvalidUrl)?;
        self.url.path = parser
            .get_str_static(start, current_slash)
            .ok_or(ErrorKind::InvalidUrl)?;

        Ok(end)
    }

    #[inline]
    fn chekc_empty_segment(
        flag: &mut bool,
        pos: usize,
        last_slash: usize,
    ) -> Result<(), ErrorKind> {
        if *flag {
            return Err(ErrorKind::DoubleSlash);
        }

        if pos == last_slash + 1 {
            *flag = true;
        }

        Ok(())
    }

    #[inline]
    fn add_url_part(&mut self, parser: &Parser, start: usize, end: usize) -> Result<(), ErrorKind> {
        if self.url.parts.len() >= self.url.parts.capacity() {
            return Err(ErrorKind::InvalidUrl);
        }

        let real_start = start + 1;
        if real_start < end {
            let slice = parser
                .get_str_static(real_start, end)
                .ok_or(ErrorKind::InvalidUrl)?;

            self.url.parts.push(slice);
        }

        Ok(())
    }

    #[inline]
    fn parse_version(
        &mut self,
        parser: &Parser,
        start: usize,
        end: usize,
        has_http_09: bool,
    ) -> Result<(bool, bool), ErrorKind> {
        let real_end = end + 1;
        let slice = parser
            .get_slice(start, real_end)
            .ok_or(ErrorKind::InvalidVersion)?;

        let (version, keep_alive) = match (slice, real_end == parser.len) {
            (b" HTTP/1.1\r\n", false) => (Version::Http11, true),
            (b" HTTP/1.1\r\n", true) => return Err(ErrorKind::InvalidHeader),
            (b" HTTP/1.0\r\n", false) => (Version::Http10, false),
            (b" HTTP/1.0\r\n", true) => return Err(ErrorKind::InvalidHeader),

            #[rustfmt::skip]
            ([rest @ .., b'\r', b'\n'], true) if
                has_http_09 && rest.len() <= 1 && rest != b" " => 
            {
                let keep_alive = self.url().path_segment(0) == Some(b"keep_alive");

                if keep_alive {
                    self.url.skip_first_segment = true;
                }

                (Version::Http09, keep_alive)
            }
            _ => return Err(ErrorKind::UnsupportedVersion),
        };

        self.version = version;

        Ok((self.version == Version::Http09, keep_alive))
    }
}

// Parse headers
impl Request {
    #[inline]
    fn parse_header(
        &mut self,
        parser: &Parser,
        req_limits: &ReqLimits,
        [start, colon, end]: [usize; 3],
    ) -> Result<(), ErrorKind> {
        let name = parser
            .get_str_static(start, colon)
            .filter(|slice| !slice.is_empty() && slice.len() <= req_limits.header_name_size)
            .ok_or(ErrorKind::InvalidHeader)?;

        let value = parser
            .get_str_static(colon + 2, end - 1)
            .filter(|slice| slice.len() <= req_limits.header_value_size)
            .ok_or(ErrorKind::InvalidHeader)?;

        // There hasn't been a `simdutf8` check yet, the data might not be `UTF-8`,
        // which will lead to UB. That's why the code works with `&[u8]`
        match name.as_bytes() {
            #[rustfmt::skip]
            [
                b'c' | b'C',
                b'o' | b'O',
                b'n' | b'N',
                b'n' | b'N',
                b'e' | b'E',
                b'c' | b'C',
                b't' | b'T',
                b'i' | b'I',
                b'o' | b'O',
                b'n' | b'N'
            ] => self.parse_header_connection(value.as_bytes())?,
            #[rustfmt::skip]
            [
                b'c' | b'C',
                b'o' | b'O',
                b'n' | b'N',
                b't' | b'T',
                b'e' | b'E',
                b'n' | b'N',
                b't' | b'T',
                b'-',
                b'l' | b'L',
                b'e' | b'E',
                b'n' | b'N',
                b'g' | b'G',
                b't' | b'T',
                b'h' | b'H'
            ] => self.parse_header_content_length(req_limits, value.as_bytes())?,
            _ => {
                let header = Header { name, value };
                self.headers.push(header);
            }
        }

        Ok(())
    }

    #[inline]
    fn parse_header_content_length(
        &mut self,
        req_limits: &ReqLimits,
        value: &[u8],
    ) -> Result<(), ErrorKind> {
        let len = types::slice_to_usize(value).ok_or(ErrorKind::InvalidContentLength)?;

        if len > req_limits.body_size {
            return Err(ErrorKind::BodyTooLarge);
        }
        self.content_length = Some(len);
        Ok(())
    }

    #[inline]
    fn parse_header_connection(&mut self, value: &[u8]) -> Result<(), ErrorKind> {
        match value {
            #[rustfmt::skip]
            [
                b'k' | b'K',
                b'e' | b'E',
                b'e' | b'E',
                b'p' | b'P',
                b'-',
                b'a' | b'A',
                b'l' | b'L',
                b'i' | b'I',
                b'v' | b'V',
                b'e' | b'E'
            ] => self.keep_alive = true,
            #[rustfmt::skip]
            [
                b'c' | b'C',
                b'l' | b'L',
                b'o' | b'O',
                b's' | b'S',
                b'e' | b'E'
            ] => self.keep_alive = false,
            _ => return Err(ErrorKind::InvalidConnection),
        }

        Ok(())
    }
}

// Parse body
impl Request {
    #[inline]
    fn process_body(&mut self, parser: &Parser, start: usize) -> Result<(), ErrorKind> {
        let body_len = parser.len - start;

        match (self.content_length, body_len) {
            (Some(0), 0) => Ok(()),
            (Some(len), available) if len == available => {
                let slice =
                    parser
                        .get_slice_static(start, parser.len)
                        .ok_or(ErrorKind::BodyMismatch {
                            expected: len,
                            available,
                        })?;

                self.body = Some(slice);
                Ok(())
            }
            (Some(len), available) => Err(ErrorKind::BodyMismatch {
                expected: len,
                available,
            }),
            (None, 0) => Ok(()),
            (None, available) => Err(ErrorKind::UnexpectedBody(available)),
        }
    }
}

//

#[derive(Debug, Clone, PartialEq)]
#[repr(align(64))]
pub(crate) struct Parser {
    len: usize,
    buffer: Box<[u8]>,
}

impl Parser {
    #[inline(always)]
    pub(crate) fn new(limits: &ReqLimits) -> Self {
        let buffer = vec![0; limits.precalc.buffer].into_boxed_slice();

        Parser { len: 0, buffer }
    }

    #[inline]
    // For tests
    pub(crate) fn from<V: AsRef<[u8]>>(limits: &ReqLimits, value: V) -> Self {
        let mut buffer = vec![0; limits.precalc.buffer];

        let value = value.as_ref();
        buffer[0..value.len()].copy_from_slice(value);

        Parser {
            len: value.len(),
            buffer: buffer.into_boxed_slice(),
        }
    }
    // For tests

    #[inline]
    pub(crate) fn reset(&mut self) {
        self.len = 0;
        self.buffer.fill(0);
    }
}

// Work with Buffer
impl Parser {
    // High level
    #[inline]
    pub(crate) async fn fill_buffer(
        &mut self,
        stream: &mut TcpStream,
        time: Duration,
    ) -> Result<usize, io::Error> {
        tokio::select! {
            biased;

            read_result = stream.read(&mut self.buffer) => {
                let n = read_result?;
                self.len = n;
                Ok(n)
            }
            _ = sleep(time) => {
                Err(io::Error::new(io::ErrorKind::TimedOut, "read timeout"))
            },
        }
    }

    #[inline]
    fn check_utf8(&self, end: usize) -> Result<(), ErrorKind> {
        simdutf8::basic::from_utf8(&self.buffer[..end])
            .map(|_| ())
            .map_err(|_| ErrorKind::InvalidEncoding)
    }

    // Search level
    #[inline]
    fn find_next_crlf<I: Iterator<Item = usize>>(
        &self,
        iter: &mut I,
        max_len_line: usize,
    ) -> Option<usize> {
        iter.next()
            .filter(|&i| i < max_len_line)
            .filter(|&i| self.get_slice(i - 1, i + 1) == Some(b"\r\n"))
    }

    #[inline]
    fn find_next_byte<I: Iterator<Item = usize>>(&self, iter: &mut I, byte: u8) -> Option<usize> {
        iter.next().filter(|&i| self.get_byte(i) == Some(byte))
    }

    // Low level
    #[inline(always)]
    fn get_slice(&self, start: usize, end: usize) -> Option<&[u8]> {
        self.buffer.get(start..end)
    }

    #[inline(always)]
    fn get_byte(&self, index: usize) -> Option<u8> {
        self.buffer.get(index).copied()
    }

    // Unsafe level
    #[inline(always)]
    // SAFETY: At the end of header parsing, the entire request (excluding the body)
    // validated using `simdutf8`, which ensures that all data is UTF-8. A request
    // that fails the simdutf8 check is invalid and must be cleared using `.reset()`.
    // These conditions guarantee the validity of &str. For information on lifetime,
    // see `Parser::into_static`.
    // DO NOT SUGGEST FIXES without full server architecture context.
    fn get_str_static(&self, start: usize, end: usize) -> Option<&'static str> {
        let value = self.buffer.get(start..end)?;
        unsafe {
            let value_str = str::from_utf8_unchecked(value);
            Some(Self::into_static(value_str))
        }
    }

    #[inline(always)]
    fn get_slice_static(&self, start: usize, end: usize) -> Option<&'static [u8]> {
        let value = self.buffer.get(start..end)?;
        unsafe { Some(Parser::into_static(value)) }
    }

    #[inline(always)]
    // SAFETY: into_static creates "temporary" references for tokio integration,
    // which become invalid after Request cleanup.
    // Parser: 'static (lives for entire program lifetime), buffer cleared via `.fill(0)`.
    // Memory remains valid even if user holds references.
    // DO NOT SUGGEST FIXES without full server architecture context.
    const unsafe fn into_static<T: ?Sized>(src: &T) -> &'static T {
        // Second `unsafe` for integration with the 2024 edition
        unsafe { mem::transmute(src) }
    }
}

#[cfg(test)]
mod request_self {
    use super::*;
    use crate::limits::Http09Limits;

    #[test]
    fn reset() {
        let limits = ReqLimits::default();
        let mut t =
            HttpConnection::from_req("OPTIONS /qwe&q=1 HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n");

        assert_eq!(t.parse_request(), Ok(()));
        t.request.reset();
        assert_eq!(Request::new(&limits), t.request);
    }

    #[test]
    fn parse_method() {
        #[rustfmt::skip]
        let cases = [
            ("GET /url\r\n",       Some(Method::Get)),
            ("GET /url\r\n",       Some(Method::Get)),
            ("PUT /url\r\n",       Some(Method::Put)),
            ("POST /url\r\n",      Some(Method::Post)),
            ("HEAD /url\r\n",      Some(Method::Head)),
            ("PATCH /url\r\n",     Some(Method::Patch)),
            ("DELETE /url\r\n",    Some(Method::Delete)),
            ("OPTIONS /url\r\n",   Some(Method::Options)),

            (" GET /url\r\n",       None),
            ("PYU /url\r\n",       None),
            ("GETGETGET /url\r\n", None),
        ];

        for (method, expected) in cases {
            let mut t = HttpConnection::from_req(method);
            t.http_09_limits = Some(Http09Limits::default());

            if let Some(expected) = expected {
                assert_eq!(t.parse_request(), Ok(()));
                assert_eq!(t.request.method(), expected);
            } else {
                assert_eq!(t.parse_request(), Err(ErrorKind::InvalidMethod));
            }
        }
    }

    #[test]
    fn parse_url() {
        #[rustfmt::skip]
        let cases = [
            ("/",      Ok((vec![], vec![]))),
            ("/?",     Ok((vec![], vec![]))),
            ("/?/",    Ok((vec![], vec![("/", "")]))),
            ("/????",  Ok((vec![], vec![("???", "")]))),
            ("/?/???", Ok((vec![], vec![("/???", "")]))),

            ("//",        Err(ErrorKind::DoubleSlash)),
            ("///api",    Err(ErrorKind::DoubleSlash)),
            ("/api//",    Err(ErrorKind::DoubleSlash)),
            ("//api///?", Err(ErrorKind::DoubleSlash)),

            (
                "/api/find?user=qwe&id=223", Ok((vec!["api", "find"],
                    vec![("user", "qwe"), ("id", "223")]
                ))
            ),
            (
                "/?user=qwe&id=223", Ok((vec![],
                    vec![("user", "qwe"), ("id", "223")]
                ))
            ),
            (
                "/?debug&name=&=Qwe&key=sda&&id=123", Ok((vec![],
                    vec![
                        ("debug", ""), ("name", ""), ("", "Qwe"),
                        ("key", "sda"), ("", "Qwe"), ("id", "123")
                    ]
                ))
            ),
            (
                "/?a=1&a=2&a=3",
                Ok((vec![], vec![("a", "1"), ("a", "1"), ("a", "1")]))
            ),
            (
                "/?very=long=value=with=equals",
                Ok((vec![], vec![("very", "long=value=with=equals")]))
            ),

            ("qwe",     Err(ErrorKind::InvalidUrl)),
            // I don't know why this is so 0_0
            ("/qwe ",   Err(ErrorKind::UnsupportedVersion)),
            (" ",       Err(ErrorKind::InvalidUrl)),
            (" /qwe", Err(ErrorKind::InvalidUrl)),
            ("qwe/qwe", Err(ErrorKind::InvalidUrl)),
        ];

        for (url, expected) in cases {
            let http_any = [
                format!("GET {url} HTTP/1.1\r\n\r\n"),
                format!("GET {url}\r\n"),
            ];

            for i in &http_any {
                let mut t = HttpConnection::from_req(i);
                t.http_09_limits = Some(Http09Limits::default());

                if let Ok((url, query)) = &expected {
                    assert_eq!(t.parse_request(), Ok(()));

                    url.iter().enumerate().for_each(|(i, value)| {
                        assert_eq!(t.request.url().path_segment_str(i), Some(*value));
                    });
                    assert_eq!(t.request.url().path_segment_str(url.len()), None);

                    query.iter().for_each(|(name, value)| {
                        assert_eq!(t.request.url().query_str(name), Some(*value));
                    });
                } else if let Err(e) = &expected {
                    assert_eq!(t.parse_request(), Err(e.clone()));
                }
            }
        }
    }

    #[test]
    fn parse_url_full() {
        let cases = [
            "GET /api/users/123?sort=name&debug HTTP/1.1\r\n\r\n",
            "GET /api/users/123?sort=name&debug HTTP/1.0\r\n\r\n",
            "GET /api/users/123?sort=name&debug\r\n",
            "GET /keep_alive/api/users/123?sort=name&debug\r\n",
        ];

        for data in cases {
            let mut t = HttpConnection::from_req(data);
            t.http_09_limits = Some(Http09Limits::default());

            let segments = &["api", "users", "123"];

            assert_eq!(t.parse_request(), Ok(()));

            assert_eq!(
                t.request.url().target_str(),
                "/api/users/123?sort=name&debug"
            );
            assert_eq!(t.request.url().path_str(), "/api/users/123");

            assert!(t.request.url().matches_str(segments));
            assert!(t.request.url().starts_with_str(segments));
            assert!(t.request.url().starts_with_str(&segments[..1]));
            assert!(t.request.url().starts_with_str(&[]));
            assert!(t.request.url().ends_with_str(segments));
            assert!(t.request.url().ends_with_str(&segments[1..]));
            assert!(t.request.url().ends_with_str(&[]));

            assert_eq!(t.request.url().path_segments_str(), segments);
            assert_eq!(t.request.url().path_segment_str(0), Some("api"));
            assert_eq!(t.request.url().path_segment_str(1), Some("users"));
            assert_eq!(t.request.url().path_segment_str(2), Some("123"));
            assert_eq!(t.request.url().path_segment_str(3), None);

            assert_eq!(t.request.url().query_full_str(), Some("?sort=name&debug"));
            assert_eq!(t.request.url().query_str("sort"), Some("name"));
            assert_eq!(t.request.url().query_str("debug"), Some(""));
        }
    }

    #[test]
    fn parse_version() {
        #[rustfmt::skip]
        let cases = [
            ("GET / HTTP/1.1\r\n\r\n", Ok(Version::Http11)),
            ("GET / HTTP/1.0\r\n\r\n", Ok(Version::Http10)),
            ("GET /\r\n",              Ok(Version::Http09)),

            ("GET / HTTP/1.1\n\n",     Err(ErrorKind::InvalidVersion)),
            ("GET / HTTP/1.0\r\r",     Err(ErrorKind::InvalidVersion)),
            ("GET /\n",                Err(ErrorKind::InvalidVersion)),
            ("GET /\r",                Err(ErrorKind::InvalidVersion)),

            ("GET / HTTP/1.1\r",       Err(ErrorKind::InvalidVersion)),
            ("GET / HTTP/1.0\n",       Err(ErrorKind::InvalidVersion)),
            ("GET / HTTP/1.1\r \n",    Err(ErrorKind::InvalidVersion)),

            ("GET / HTTP/1.0 \r\n",    Err(ErrorKind::UnsupportedVersion)),
            ("GET / HTTP/2.0\r\n\r\n", Err(ErrorKind::UnsupportedVersion)),
            ("GET / HTTP/0.9\r\n\r\n", Err(ErrorKind::UnsupportedVersion)),
            ("GET / http/1.1\r\n\r\n", Err(ErrorKind::UnsupportedVersion)),
            ("GET / HTTP/1.15\r\n",    Err(ErrorKind::UnsupportedVersion)),
        ];

        for (value, expected) in cases {
            let mut t = HttpConnection::from_req(value);
            t.http_09_limits = Some(Http09Limits::default());

            if let Ok(version) = expected {
                assert_eq!(t.parse_request(), Ok(()));

                assert_eq!(t.request.version, version);

                match t.request.version {
                    Version::Http11 => assert!(t.request.is_keep_alive()),
                    Version::Http10 => assert!(!t.request.is_keep_alive()),
                    Version::Http09 => assert!(!t.request.is_keep_alive()),
                }
            } else if let Err(e) = expected {
                assert_eq!(t.parse_request(), Err(e));
            }
        }
    }

    #[test]
    fn parse_header() {
        #[rustfmt::skip]
        let cases = [
            ("HEADER: value\r\n", Some(("header", "value"))),
            ("Header: value\r\n", Some(("header", "value"))),
            ("header: value\r\n", Some(("header", "value"))),
            ("header:   value  \r\n", Some(("header", "  value  "))),
            ("header: \r\n",      Some(("header", ""))),

            ("Header: value\r",   None),
            ("Header: value",     None),
            ("header:value\n",    None),
            ("header:\n",         None),
            (": value\r\n",       None),
            (": \r\n",            None),
            (": value\n",         None),
            (": \n",              None),
        ];

        for (header, expected) in cases {
            let mut t = HttpConnection::from_req(format!("GET / HTTP/1.1\r\n{header}\r\n"));

            if let Some((name, value)) = expected {
                assert_eq!(t.parse_request(), Ok(()));

                assert_eq!(t.request.header_str(name), Some(value));
            } else {
                assert_eq!(t.parse_request(), Err(ErrorKind::InvalidHeader));
            }
        }
    }

    #[test]
    fn parse_headers() {
        #[rustfmt::skip]
        let cases = [
            (
                "HEADER: value\r\n\r\n",
                Some((vec!["HEADER", "HeAdEr", "header"], "value")),
            ),
            ("HEADER: value\n\n",  None),
            (
                "HEADER: value\r\nQwE: value\r\nasd: value\r\n\r\n",
                Some((vec!["header", "qwe", "asd"], "value")),
            ),
            ("HEADER: value\nQwE: value\nasd: value\n\n", None),
            (
                "Empty-Value: \r\n\r\n",
                Some((vec!["empty-value"], "")),
            ),
            (
                "Space-Value:   \r\n\r\n",
                Some((vec!["space-value"], "  ")),
            ),
            (
                "Multi: value1\r\nMulti: value2\r\n\r\n",
                Some((vec!["multi"], "value1")),
            ),

            (": empty-name\r\n\r\n", None),
            ("No-Colon value\r\n\r\n", None),
            ("Valid: ok\r\nInvalidname\r\nNext: value\r\n\r\n", None),
            ("Header: value\n\n", None),
            ("No-Colon value\r\n\r\n", None),
            ("Valid: ok\r\nInvalidname\r\nNext: value\r\n\r\n", None),
        ];

        for (headers, expected) in cases {
            let mut t = HttpConnection::from_req(format!("GET / HTTP/1.1\r\n{headers}"));

            if let Some((names, value)) = expected {
                assert_eq!(t.parse_request(), Ok(()));

                for name in names {
                    assert_eq!(t.request.header_str(name), Some(value));
                }
            } else {
                assert_eq!(t.parse_request(), Err(ErrorKind::InvalidHeader));
            }
        }
    }

    #[test]
    fn parse_special_header() {
        #[rustfmt::skip]
        let cases = [
            ("content-length: 6\r\n\r\n123456", Ok((Some(6), None))),
            (
                "content-length: 3\r\nconnection: keep-alive\r\n\r\n123",
                Ok((Some(3), Some(true)))
            ),
            ("connection: keep-alive\r\n\r\n", Ok((None, Some(true)))),
            (
                "content-length: 4\r\nconnection: close\r\n\r\n1234",
                Ok((Some(4), Some(false)))
            ),
            ("connection: close\r\n\r\n", Ok((None, Some(false)))),

            ("connection: keep_alive\r\n\r\n", Err(ErrorKind::InvalidConnection)),
            ("connection: qwerrew\r\n\r\n", Err(ErrorKind::InvalidConnection)),
            ("content-length: 12asd\r\n\r\n", Err(ErrorKind::InvalidContentLength)),
            ("content-length: 123u64\r\n\r\n", Err(ErrorKind::InvalidContentLength)),
            ("content-length: 4097\r\n\r\n", Err(ErrorKind::BodyTooLarge)),
            ("content-length: 123.9435\r\n\r\n", Err(ErrorKind::InvalidContentLength)),
            (
                "content-length: 999999999999999999999\r\n\r\n",
                Err(ErrorKind::InvalidContentLength)
            ),
        ];

        for (headers, result) in cases {
            let mut t = HttpConnection::from_req(format!("GET / HTTP/1.1\r\n{headers}"));

            if let Ok((content_length, keep_alive)) = result {
                assert_eq!(t.parse_request(), Ok(()));
                assert!(t.request.headers.is_empty());

                if let Some(len) = content_length {
                    assert_eq!(t.request.content_length(), Some(len));
                }
                if let Some(keep_alive) = keep_alive {
                    assert_eq!(t.request.is_keep_alive(), keep_alive);
                }
            } else if let Err(e) = result {
                assert_eq!(t.parse_request(), Err(e));
            }
        }
    }

    macro_rules! parse_request {
        ($cases:expr) => {
            for (req, result) in $cases {
                let mut t = HttpConnection::from_req(req);

                if let Ok(result) = result {
                    assert_eq!(t.parse_request(), Ok(()));

                    assert_eq!(t.request.version(), result.2);
                    assert_eq!(t.request.method(), result.0);
                    assert_eq!(t.request.url().target_str(), result.1);

                    for (name, value) in result.3 {
                        assert_eq!(
                            t.request.header_str(name),
                            Some(value.to_string()).as_deref()
                        );
                    }
                    assert_eq!(t.request.body(), result.4);
                    assert_eq!(t.request.is_keep_alive(), result.5);
                } else if let Err(e) = result {
                    assert_eq!(t.parse_request(), Err(e));
                }
            }
        };
    }

    #[test]
    fn parse_valid_request() {
        #[rustfmt::skip]
        let cases = vec![
            (
                "GET / HTTP/1.1\r\n\r\n",
                Ok((
                    Method::Get, "/", Version::Http11,
                    vec![],
                    None, true
                )),
            ),
            (
                "GET /api/qwe/name/len/qwe HTTP/1.1\r\n\r\n",
                Ok((
                    Method::Get, "/api/qwe/name/len/qwe", Version::Http11,
                    vec![],
                    None, true
                )),
            ),
            ("GET / HTTP/1.1\n\n", Err(ErrorKind::InvalidVersion)),
            (
                "POST /test HTTP/1.1\r\nHOST: 127.0.0.1\r\n\r\n",
                Ok((
                    Method::Post, "/test", Version::Http11,
                    vec![("host", "127.0.0.1")],
                    None, true,
                )),
            ),
            (
                "PUT /qwe HTTP/1.1\r\nHoSt: 127.0.0.1\r\nUser-Agent: curl\r\n\r\n",
                Ok((
                    Method::Put, "/qwe", Version::Http11,
                    vec![("host", "127.0.0.1"), ("user-agent", "curl")],
                    None, true,
                )),
            ),
            (
                "GET /file HTTP/1.1\r\ncontent-length: 12\r\n\r\nHello world!",
                Ok((
                    Method::Get, "/file", Version::Http11,
                    vec![],
                    Some(b"Hello world!" as &[u8]), true,
                )),
            ),
            (
                "HEAD / HTTP/1.1\r\nConnection: keep-alive\r\n\r\n",
                Ok((
                    Method::Head, "/", Version::Http11,
                    vec![],
                    None, true
                )),
            ),
            (
                "OPTIONS / HTTP/1.1\r\nCoNNEctIon: close\r\n\r\n",
                Ok((
                    Method::Options, "/", Version::Http11,
                    vec![],
                    None, false
                )),
            ),
            (
                "PATCH / HTTP/1.0\r\nconnection: keep-alive\r\n\r\n",
                Ok((
                    Method::Patch, "/", Version::Http10,
                    vec![],
                    None, true
                )),
            ),
            (
                "DELETE / HTTP/1.0\r\nConnection: close\r\n\r\n",
                Ok((
                    Method::Delete, "/", Version::Http10,
                    vec![],
                    None, false
                )),
            ),
            (
                "GET / HTTP/1.0\r\n\r\n",
                Ok((
                    Method::Get, "/", Version::Http10,
                    vec![],
                    None, false
                )),
            ),
            (
    "POST /upload HTTP/1.1\r\nContent-Type: application/json\r
Content-Length: 17\r\n\r\n{\"data\": \"value\"}",
                Ok((
                    Method::Post, "/upload", Version::Http11,
                    vec![("content-type", "application/json")],
                    Some(b"{\"data\": \"value\"}" as &[u8]), true,
                )),
            ),
            (
                "GET /empty HTTP/1.1\r\nX-Empty: \r\nX-Space: \r\n\r\n",
                Ok((
                    Method::Get, "/empty", Version::Http11,
                    vec![("x-empty", ""), ("x-space", "")],
                    None, true,
                )),
            ),
        ];

        parse_request! { cases }
    }

    #[test]
    fn parse_invalid_request() {
        #[rustfmt::skip]
        let cases = vec![
            (
                "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                Ok((
                    Method::Get, "/", Version::Http11,
                    vec![("Host", "127.0.0.1")],
                    None::<&[u8]>, true
                )),
            ),
            (
                " GET/ HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                Err(ErrorKind::InvalidMethod)
            ),
            (
                "GET/ HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                Err(ErrorKind::InvalidMethod)
            ),
            (
                "GET",
                Err(ErrorKind::InvalidVersion)
            ),
            (
                "GET ",
                Err(ErrorKind::InvalidVersion)
            ),
            (
                "GET  HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                Err(ErrorKind::InvalidUrl)
            ),
            (
                "GET /\r\n", // Without HTTTP/0.9+ limits
                Err(ErrorKind::UnsupportedVersion)
            ),
            (
                "GET /HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                Err(ErrorKind::InvalidUrl)
            ),
            (
                "GET / HTTP/1.1 \r\nHost: 127.0.0.1\r\n\r\n",
                Err(ErrorKind::UnsupportedVersion)
            ),
            (
                "GET / HTTP/1.1\r\nHost: 127.0.0.1\n\r\n",
                Err(ErrorKind::InvalidHeader)
            ),
            (
                "GET / HTTP/1.1\nHost: 127.0.0.1\r\n\r\n",
                Err(ErrorKind::InvalidVersion)
            ),
            (
                "GET / HTTP/1.1\r\nHost: 127.0.0.1\nq: w\r\n\r\n",
                Err(ErrorKind::InvalidHeader)
            ),
            (
                "GET /empty HTTP/1.1\r\nX-Empty:\r\nX-Space: \r\n\r\n",
                Err(ErrorKind::InvalidHeader)
            ),
            (
                "GET / HTTP/1.1\r\nQ: w\n\n",
                Err(ErrorKind::InvalidHeader)
            ),
            (
                "GET / HTTP/1.1\r\nQ: w\r\nW: w\n\n",
                Err(ErrorKind::InvalidHeader)
            ),
            (
                "POST / HTTP/1.1\r\nContent-Length: 10\r\n\r\nshort",
                Err(ErrorKind::BodyMismatch { expected: 10, available: 5 }),
            ),
            (
                "POST / HTTP/1.1\r\nContent-Length: 999999999\r\n\r\nbody",
                Err(ErrorKind::BodyTooLarge),
            ),
            (
                "POST / HTTP/1.1\r\nContent-Length: invalid\r\n\r\nbody",
                Err(ErrorKind::InvalidContentLength),
            ),
        ];

        parse_request! { cases }
    }

    #[test]
    fn check_limits() {
        use crate::query::Error as Qerror;

        let limits = ReqLimits::default().precalculate();

        let def_url = "/".to_string();
        let url_size = format!("/{}", "q".repeat(limits.url_size - 1));
        let url_parts = "/q".repeat(limits.url_parts);
        let url_query_size = format!("/?{}", "q".repeat(limits.url_query_size - 1));
        let url_query_parts = format!("/?{}", vec!["q=w"; limits.url_query_parts].join("&"));

        let h_name = "N".repeat(limits.header_name_size);
        let h_value = "v".repeat(limits.header_value_size);

        let body = "b".repeat(limits.body_size);

        #[rustfmt::skip]
        let cases = vec![
            (
                format!("GET {url_size} HTTP/1.1\r\n\r\n"),
                Ok((
                    Method::Get, &url_size, Version::Http11,
                    vec![],
                    None, true
                )),
            ),
            (
                format!("GET {url_size}e HTTP/1.1\r\n\r\n"),
                Err(ErrorKind::InvalidUrl),
            ),
            (
                format!("GET {url_parts} HTTP/1.1\r\n\r\n"),
                Ok((
                    Method::Get, &url_parts, Version::Http11,
                    vec![],
                    None, true
                )),
            ),
            (
                format!("GET {url_query_size} HTTP/1.1\r\n\r\n"),
                Ok((
                    Method::Get, &url_query_size, Version::Http11,
                    vec![],
                    None, true
                )),
            ),
            (
                format!("GET {url_query_size}e HTTP/1.1\r\n\r\n"),
                Err(ErrorKind::InvalidUrl),
            ),
            (
                format!("GET {url_parts}/e HTTP/1.1\r\n\r\n"),
                Err(ErrorKind::InvalidUrl),
            ),
            (
                format!("GET {} HTTP/1.1\r\n\r\n", url_query_parts),
                Ok((
                    Method::Get, &url_query_parts, Version::Http11,
                    vec![],
                    None, true
                )),
            ),
            (
                format!(
                    "GET /?{} HTTP/1.1\r\n\r\n",
                    vec!["q=w"; limits.url_query_parts + 1].join("&")
                ),
                Err(ErrorKind::Query(
                    Qerror::OverLimit(limits.url_query_parts)
                )),
            ),
                (
                format!("GET / HTTP/1.1\r\n{h_name}: {h_value}\r\n\r\n"),
                Ok((
                    Method::Get, &def_url, Version::Http11,
                    vec![(&h_name, &h_value)],
                    None, true
                )),
            ),
            (
                format!("GET / HTTP/1.1\r\n{h_name}e: value\r\n\r\n"),
                Err(ErrorKind::InvalidHeader),
            ),
            (
                format!("GET / HTTP/1.1\r\nName: {h_value}e\r\n\r\n"),
                Err(ErrorKind::InvalidHeader),
            ),
            (
                format!(
                    "GET / HTTP/1.1\r\n{}\r\n",
                    format!("{h_name}: {h_value}\r\n")
                        .repeat(limits.header_count)
                ),
                Ok((
                    Method::Get, &def_url, Version::Http11,
                    vec![(&h_name, &h_value); limits.header_count],
                    None, true
                )),
            ),
            (
                format!(
                    "GET / HTTP/1.1\r\n{}\r\n",
                    format!("{h_name}: {h_value}\r\n")
                        .repeat(limits.header_count + 1)
                ),
                Err(ErrorKind::TooManyHeaders),
            ),
            (
                format!(
                    "GET / HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
                    limits.body_size
                ),
                Ok((
                    Method::Get, &def_url, Version::Http11,
                    vec![],
                    Some(body.as_bytes()), true
                )),
            ),
            (
                format!(
                    "GET / HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}e",
                    limits.body_size + 1,
                ),
                Err(ErrorKind::BodyTooLarge),
            ),

            (
                format!(
        "OPTIONS {url_size} HTTP/1.1\r\nContent-Length: {}\r\n{}\r\n{body}",
                    limits.body_size,
                    &format!("{h_name}: {h_value}\r\n")
                        .repeat(limits.header_count - 1)[22..]
                ),
                Ok((
                    Method::Options, &url_size, Version::Http11,
                    vec![(&h_name, &h_value); limits.header_count - 1],
                    Some(body.as_bytes()), true
                ))
            )
        ];

        parse_request! { cases }
    }
}
