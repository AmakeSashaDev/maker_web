use crate::{
    errors::*,
    http::types::{self, Header, HeaderMap},
    limits::ReqLimits,
    query::Query,
    server::connection::HttpConnection,
    ConnectionData, Handler, Method, Url, Version,
};
use memchr::{memchr, memchr_iter};
use std::{io, mem, time::Duration};
use tokio::{io::AsyncReadExt, net::TcpStream, time::sleep};

/// High-performance HTTP request representation.
///
/// Uses strategic memory alignment for optimal cache performance.
/// All data is zero-copy referenced from the original input.
#[derive(Debug, Clone, PartialEq)]
#[repr(align(128))]
pub struct Request {
    method: Method,
    url: Url,
    version: Version,
    headers: HeaderMap,
    body: Option<&'static [u8]>,
}

impl Request {
    #[inline(always)]
    pub(crate) fn new(limits: &ReqLimits) -> Self {
        Request {
            method: Method::Get,
            url: Url::new(limits),
            version: Version::Http11,
            headers: HeaderMap::new(limits.header_count),
            body: None,
        }
    }

    #[inline(always)]
    pub(crate) fn reset(&mut self) {
        self.method = Method::Get;
        self.url.clear();
        self.version = Version::Http11;
        self.headers.reset();
        self.body = None;
    }
}

// Public API
impl Request {
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
    ///
    /// # Arguments
    /// - `name`: Header name bytes (e.g., `b"content-type"`)
    #[inline(always)]
    pub fn header(&self, name: &[u8]) -> Option<&[u8]> {
        self.headers.get(name)
    }

    /// Returns the value of the `Content-Length` header if present.
    #[inline(always)]
    pub const fn content_length(&self) -> Option<usize> {
        self.headers.content_length
    }

    /// Returns the request body if present.
    #[inline(always)]
    pub const fn body(&self) -> Option<&[u8]> {
        self.body
    }
}

// If you don't like using HttpConnection instead of transmitting all the
// values, then you can't even imagine what happened here...
// It's a pity now that you can understand this code (when passing all the
//  values, I didn't understand it myself):(
impl<H: Handler<S>, S: ConnectionData> HttpConnection<H, S> {
    #[inline]
    pub(crate) fn parse(&mut self) -> Result<Version, ErrorKind> {
        self.parse_method()?;
        self.parse_url()?;
        if self.check_http09()? {
            return Ok(self.request.version);
        }
        self.check_version()?;

        self.parse_headers()?;
        self.check_body()?;

        Ok(self.request.version)
    }
}

// Parse first line
impl<H: Handler<S>, S: ConnectionData> HttpConnection<H, S> {
    // Basic methods
    #[inline]
    fn parse_method(&mut self) -> Result<(), ErrorKind> {
        // "OPTIONS " - The longest possible method with a space (8 bytes)
        let slice = self
            .parser
            .get_slice(0, 8)
            .ok_or(ErrorKind::InvalidMethod)?;

        (self.request.method, self.parser.position) = Method::from_bytes(slice)?;
        Ok(())
    }

    #[inline]
    // Cannot replace with `get_slice` or `find_slice` method
    // due to ownership error. And there is no need to use `into_static`
    fn parse_url(&mut self) -> Result<(), ErrorKind> {
        let [start, posit] = match self
            .parser
            .find_char(self.req_limits.precalc.url_size_memchr, b' ')
        {
            Some(pos) => [self.parser.position + 1, pos],
            None if self.is_http_09() => self.get_range_url_http_09()?,
            None => return Err(ErrorKind::InvalidUrl),
        };

        let parser = &mut self.parser;
        let url = &mut self.request.url;

        let mut last = 0;
        let start_pos = start + posit;
        let slice_url = &parser.buffer[start..start_pos];

        if slice_url.is_empty() || parser.buffer[start - 1] != b'/' {
            return Err(ErrorKind::InvalidUrl);
        }

        for index in memchr_iter(b'/', slice_url) {
            if url.parts.len() == self.req_limits.url_parts {
                return Err(ErrorKind::InvalidUrl);
            }

            let slice_part = parser
                .get_slice_static(start + last, index - last)
                .ok_or(ErrorKind::InvalidUrl)?;

            if !slice_part.is_empty() {
                url.parts.push(slice_part);
            }
            last = index + 1;
        }

        let range = [start + last, (posit - last).saturating_sub(1)];
        let (end, url_middle) = match parser.find_char(posit, b'?') {
            Some(q_pos) => {
                let slice = parser
                    .get_slice_static(q_pos, (range[0] + range[1]).saturating_sub(q_pos))
                    .ok_or(ErrorKind::InvalidUrl)?;
                let limit = url.query_parts.capacity();

                Query::parse_into(&mut url.query_parts, slice, limit)?;
                url.query = Some(slice);

                (q_pos, q_pos)
            }
            None => (posit, posit),
        };

        let slice = parser
            .get_slice_static(range[0], (parser.position + end).saturating_sub(range[0]))
            .ok_or(ErrorKind::InvalidUrl)?;

        if !slice.is_empty() {
            url.parts.push(slice);
        }

        url.path = parser
            .get_slice_static(parser.position, url_middle)
            .ok_or(ErrorKind::InvalidUrl)?;
        url.target = parser
            .get_slice_static(parser.position, posit)
            .ok_or(ErrorKind::InvalidUrl)?;

        parser.update_position(posit);

        Ok(())
    }

    #[inline]
    fn check_version(&mut self) -> Result<(), ErrorKind> {
        // "HTTP/1.X\r\n" - HTTP version with line break (10 bytes)
        let slice = self
            .parser
            .find_slice(10, b'\n')
            .ok_or(ErrorKind::InvalidVersion)?;

        if !matches!(slice.len(), 8 | 9) {
            return Err(ErrorKind::InvalidVersion);
        }

        (self.response.version, self.response.keep_alive) = Version::from_bytes(&slice[..8])?;
        self.request.version = self.response.version;

        // Check for the use of the '\r' character
        self.parser.has_crlf = slice.last() == Some(&b'\r');

        Ok(())
    }

    // Auxiliary methods
    #[inline(always)]
    fn is_http_09(&self) -> bool {
        self.http_09_limits.is_some()
            && self.parser.len < self.req_limits.precalc.len_http09
            && self.request.method == Method::Get
            && self.parser.buffer[..self.parser.len].ends_with(b"\r\n")
    }

    #[inline]
    fn get_range_url_http_09(&mut self) -> Result<[usize; 2], ErrorKind> {
        let parser = &mut self.parser;

        let end_url = parser.len - 2;
        if parser.position >= end_url {
            return Err(ErrorKind::InvalidUrl);
        }
        let slice = parser
            .get_slice(parser.position, 12)
            .ok_or(ErrorKind::InvalidUrl)?;

        if slice.starts_with(b"/keep_alive") {
            self.response.keep_alive = true;
            parser.position += 11;
        } else {
            self.response.keep_alive = false;
        };

        self.request.version = Version::Http09;
        self.response.version = Version::Http09;

        Ok([parser.position + 1, end_url - parser.position])
    }

    #[inline]
    fn check_http09(&mut self) -> Result<bool, ErrorKind> {
        if self.request.version == Version::Http09 {
            if self.http_09_limits.is_none() {
                return Err(ErrorKind::UnsupportedVersion);
            }

            let p = &self.parser;
            match &p.buffer[p.position..p.len].ends_with(b"\n") {
                true => Ok(true),
                false => Err(ErrorKind::InvalidVersion),
            }
        } else {
            Ok(false)
        }
    }
}

// Parse headers
impl<H: Handler<S>, S: ConnectionData> HttpConnection<H, S> {
    // Basic methods
    #[inline]
    fn parse_headers(&mut self) -> Result<(), ErrorKind> {
        for _ in 0..=self.req_limits.header_count {
            let Some(header) = self.parse_header()? else {
                return Ok(());
            };

            if !self.parse_special_header(&header)? {
                self.request.headers.headers.push(header);
            }
        }

        Err(ErrorKind::TooManyHeaders)
    }

    #[inline]
    fn parse_header(&mut self) -> Result<Option<Header>, ErrorKind> {
        let parser = &mut self.parser;
        // HeaderName: Someone=data\r\n
        //                            |
        let end = parser
            .find_char(self.req_limits.precalc.h_line, b'\n')
            .ok_or(ErrorKind::InvalidHeader)?;

        match parser.get_slice(parser.position + end - 1, 2) {
            Some([b'\r', b'\n']) if parser.has_crlf => {}
            Some([_, b'\n']) if !parser.has_crlf => {}
            _ => return Err(ErrorKind::InvalidHeader),
        }

        // HeaderName: Someone=data\r\n
        //           |
        let Some(split) = parser.find_char(end, b':') else {
            self.check_end_of_headers(end)?;
            return Ok(None);
        };

        if parser.get_slice(parser.position + split, 2) != Some(b": ") {
            return Err(ErrorKind::InvalidHeader);
        }

        let value_start = split + 2;
        let len_value = end - value_start - parser.has_crlf as usize;

        if split > self.req_limits.header_name_size || len_value > self.req_limits.header_value_size
        {
            return Err(ErrorKind::InvalidHeader);
        }

        let name = {
            let name = parser
                .get_slice_mut(parser.position, split)
                .ok_or(ErrorKind::InvalidHeader)?;

            if name.is_empty() {
                return Err(ErrorKind::InvalidHeader);
            }

            types::to_lower_case(name);
            unsafe { Parser::into_static(name) }
        };

        let value = parser
            .get_slice_static(parser.position + value_start, len_value)
            .ok_or(ErrorKind::InvalidHeader)?;

        parser.update_position(end);

        Ok(Some(Header::new(name, value)))
    }

    #[inline]
    fn parse_special_header(&mut self, header: &Header) -> Result<bool, ErrorKind> {
        match header.name {
            b"content-length" => self.parse_content_length(header.value),
            b"connection" => self.parse_connection(header.value),
            _ => return Ok(false),
        }
        .map(|_| true)
    }

    // Auxiliary methods
    #[inline]
    fn check_end_of_headers(&mut self, start: usize) -> Result<(), ErrorKind> {
        let parser = &mut self.parser;
        // [\r, \n, \r, \n] or [x, x, \n, \n]
        let p_end = parser
            .get_slice(parser.position + start - 3, 4)
            .ok_or(ErrorKind::InvalidHeader)?;

        if !match parser.has_crlf {
            true => p_end.ends_with(b"\r\n\r\n"),
            false => p_end.ends_with(b"\n\n"),
        } {
            return Err(ErrorKind::InvalidHeader);
        }

        parser.position += parser.has_crlf as usize + 1;

        Ok(())
    }

    #[inline]
    fn parse_content_length(&mut self, value: &[u8]) -> Result<(), ErrorKind> {
        let len = types::slice_to_usize(value).ok_or(ErrorKind::InvalidContentLength)?;
        if len > self.req_limits.body_size {
            return Err(ErrorKind::BodyTooLarge);
        }
        self.request.headers.content_length = Some(len);
        Ok(())
    }

    #[inline]
    fn parse_connection(&mut self, value: &[u8]) -> Result<(), ErrorKind> {
        let mut normalized = [0; 10];
        let len = types::into_lower_case(value, &mut normalized);

        match &normalized[..len] {
            b"keep-alive" => self.response.keep_alive = true,
            b"close" => self.response.keep_alive = false,
            _ => return Err(ErrorKind::InvalidConnection),
        }

        Ok(())
    }
}

// Parse body
impl<H: Handler<S>, S: ConnectionData> HttpConnection<H, S> {
    #[inline]
    fn check_body(&mut self) -> Result<(), ErrorKind> {
        let parser = &self.parser;
        let body = parser.len - parser.position;

        match self.request.headers.content_length {
            Some(len) if len == body => {
                let slice = parser.get_slice_static(parser.position, len).ok_or(
                    ErrorKind::BodyMismatch {
                        expected: len,
                        available: body,
                    },
                )?;

                self.request.body = Some(slice);
                Ok(())
            }
            Some(len) => Err(ErrorKind::BodyMismatch {
                expected: len,
                available: body,
            }),
            None => match body == 0 {
                true => Ok(()),
                false => Err(ErrorKind::UnexpectedBody(body)),
            },
        }
    }
}

//

#[derive(Debug, Clone, PartialEq)]
#[repr(align(64))]
pub(crate) struct Parser {
    position: usize,
    len: usize,
    has_crlf: bool,
    buffer: Box<[u8]>,
}

impl Parser {
    #[inline(always)]
    pub(crate) fn new(limits: &ReqLimits) -> Self {
        let buffer = vec![0; limits.precalc.buffer].into_boxed_slice();

        Parser {
            position: 0,
            len: 0,
            has_crlf: false,
            buffer,
        }
    }

    #[cfg(test)]
    pub(crate) fn from<V: AsRef<[u8]>>(limits: &ReqLimits, value: V) -> Self {
        let mut buffer = vec![0; limits.precalc.buffer];

        let value = value.as_ref();
        buffer[0..value.len()].copy_from_slice(value);

        Parser {
            position: 0,
            len: value.len(),
            has_crlf: false,
            buffer: buffer.into_boxed_slice(),
        }
    }

    #[inline]
    pub(crate) fn reset(&mut self) {
        self.position = 0;
        self.len = 0;
        self.has_crlf = false;
        self.buffer.fill(0);
    }
}

// Work with Buffer
impl Parser {
    // Reading level
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

    // Search level
    #[inline]
    fn find_slice(&mut self, limit: usize, delimiter: u8) -> Option<&[u8]> {
        let step = self.find_char(limit, delimiter)?;
        let last_index = self.update_position(step);
        let slice = self.get_slice(last_index, step)?;
        Some(slice)
    }

    #[inline]
    fn find_char(&self, step: usize, delimiter: u8) -> Option<usize> {
        let slice = self.get_slice(self.position, step)?;
        memchr(delimiter, slice)
    }

    // Low level
    #[inline(always)]
    fn get_slice(&self, start: usize, step: usize) -> Option<&[u8]> {
        self.buffer.get(start..start + step)
    }

    #[inline(always)]
    fn get_slice_mut(&mut self, start: usize, step: usize) -> Option<&mut [u8]> {
        self.buffer.get_mut(start..start + step)
    }

    #[inline(always)]
    fn update_position(&mut self, step: usize) -> usize {
        let old = self.position;
        self.position += step + 1;
        old
    }

    // Unsafe level
    #[inline(always)]
    fn get_slice_static(&self, start: usize, step: usize) -> Option<&'static [u8]> {
        let value = self.get_slice(start, step)?;
        unsafe { Some(Self::into_static(value)) }
    }

    #[inline(always)]
    // SAFETY: into_static creates "temporary" references for tokio integration,
    // which become invalid after Request cleanup.
    // Parser: 'static (lives for entire program lifetime), buffer cleared via .fill(0).
    // Memory remains valid even if user holds references.
    // DO NOT SUGGEST FIXES without full server architecture context.
    const unsafe fn into_static(src: &[u8]) -> &'static [u8] {
        // Second `unsafe` for integration with the 2024 edition
        unsafe { mem::transmute(src) }
    }
}

#[cfg(test)]
mod request_self {
    use super::*;
    use crate::tools::*;

    #[test]
    fn reset() {
        let limits = ReqLimits::default();
        let mut t =
            HttpConnection::from_req("OPTIONS /qwe&q=1 HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n");

        assert_eq!(t.parse(), Ok(Version::Http10));
        t.request.reset();
        assert_eq!(Request::new(&limits), t.request);
    }

    #[test]
    fn parse_method() {
        #[rustfmt::skip]
        let cases = [
            ("GET /url",   Some(Method::Get)),
            ("GET ",       Some(Method::Get)),
            ("PUT ",       Some(Method::Put)),
            ("POST ",      Some(Method::Post)),
            ("HEAD ",      Some(Method::Head)),
            ("PATCH ",     Some(Method::Patch)),
            ("DELETE ",    Some(Method::Delete)),
            ("OPTIONS ",   Some(Method::Options)),

            ("GET",        None),
            ("PYU ",       None),
            ("GETGETGET ", None),
        ];

        for (method, expected) in cases {
            let mut t = HttpConnection::from_req(method);

            if let Some(expected) = expected {
                assert_eq!(t.parse_method(), Ok(()));
                assert_eq!(t.request.method(), expected);
            } else {
                assert_eq!(t.parse_method(), Err(ErrorKind::InvalidMethod));
            }
        }
    }

    #[test]
    fn parse_url() {
        #[rustfmt::skip]
        let cases = [
            ("/ ",             Some((vec![], vec![]))),
            ("/// ",           Some((vec![], vec![]))),
            ("/? ",            Some((vec![], vec![]))),
            ("/?/ ",           Some((vec!["?"], vec![]))),
            ("/?? ",           Some((vec![], vec![]))),
            ("/???? ",         Some((vec![], vec![]))),

            ("/api/user ",     Some((vec!["api", "user"], vec![]))),
            ("///api//user ",  Some((vec!["api", "user"], vec![]))),
            ("/api/qwe/name/len ",  Some((vec!["api", "qwe", "name", "len"], vec![]))),
            ("/api//user/// ", Some((vec!["api", "user"], vec![]))),
            ("/api//user//? ", Some((vec!["api", "user"], vec![]))),

            ("/api ",          Some((vec!["api"], vec![]))),
            ("///api ",        Some((vec!["api"], vec![]))),
            ("/api// ",        Some((vec!["api"], vec![]))),
            ("//api///? ",     Some((vec!["api"], vec![]))),

            (
                "/api/find?user=qwe&id=223 ", Some((vec!["api", "find"], 
                    vec![("user", "qwe"), ("id", "223")]
                ))
            ),
            (
                "/?user=qwe&id=223 ", Some((vec![], 
                    vec![("user", "qwe"), ("id", "223")]
                ))
            ),
            (
                "/?debug&name=&=Qwe&key=sda&&id=123 ", Some((vec![], 
                    vec![
                        ("debug", ""), ("name", ""), ("", "Qwe"), 
                        ("key", "sda"), ("", "Qwe"), ("id", "123")
                    ]
                ))
            ),
            (
                "/?a=1&a=2&a=3 ",
                Some((vec![], vec![("a", "1"), ("a", "1"), ("a", "1")]))
            ),
            (
                "/?very=long=value=with=equals ",
                Some((vec![], vec![("very", "long=value=with=equals")]))
            ),

            ("qwe ",           None),
            (" ",              None),
            ("qwe/qwe ",       None),
            ("/qwe",           None),
        ];

        for (url, expected) in cases {
            let mut t = HttpConnection::from_req(url);

            if let Some((url, query)) = expected {
                assert_eq!(t.parse_url(), Ok(()));

                url.iter().enumerate().for_each(|(i, value)| {
                    assert_eq!(str(t.request.url().path_segment(i)), Some(*value));
                });
                assert_eq!(str(t.request.url().path_segment(url.len())), None);

                query.iter().for_each(|(name, value)| {
                    assert_eq!(str(t.request.url().query(name.as_bytes())), Some(*value));
                });
            } else {
                assert_eq!(t.parse_url(), Err(ErrorKind::InvalidUrl));
            }
        }
    }

    #[test]
    fn parse_url_full() {
        let mut t = HttpConnection::from_req("/api/users/123?sort=name&debug ");
        let segments = &[b"api" as &[u8], b"users" as &[u8], b"123" as &[u8]] as &[&[u8]];

        assert_eq!(t.parse_url(), Ok(()));

        assert_eq!(
            str_op(t.request.url().target()),
            "/api/users/123?sort=name&debug"
        );
        assert_eq!(str_op(t.request.url().path()), "/api/users/123");

        assert!(t.request.url().matches(segments));
        assert!(t.request.url().starts_with(segments));
        assert!(t.request.url().starts_with(&segments[..1]));
        assert!(t.request.url().starts_with(&[]));
        assert!(t.request.url().ends_with(segments));
        assert!(t.request.url().ends_with(&segments[1..]));
        assert!(t.request.url().ends_with(&[]));

        assert_eq!(t.request.url().path_segments(), segments);
        assert_eq!(str(t.request.url().path_segment(0)), Some("api"));
        assert_eq!(str(t.request.url().path_segment(1)), Some("users"));
        assert_eq!(str(t.request.url().path_segment(2)), Some("123"));
        assert_eq!(str(t.request.url().path_segment(3)), None);

        assert_eq!(str(t.request.url().query_full()), Some("?sort=name&debug"));
        assert_eq!(str(t.request.url().query(b"sort")), Some("name"));
        assert_eq!(str(t.request.url().query(b"debug")), Some(""));
    }

    #[test]
    fn check_version() {
        #[rustfmt::skip]
        let cases = [
            ("HTTP/1.1\r\n e", Ok((Version::Http11, true))),
            ("HTTP/1.1\r\n",   Ok((Version::Http11, true))),
            ("HTTP/1.0\r\n",   Ok((Version::Http10, true))),
            ("HTTP/1.1\n",     Ok((Version::Http11, false))),
            ("HTTP/1.0\n",     Ok((Version::Http10, false))),

            ("HTTP/2.0\r\n",   Err(ErrorKind::UnsupportedVersion)),
            ("HTTP/0.9\r\n",   Err(ErrorKind::UnsupportedVersion)),
            ("http/1.1\r\n",   Err(ErrorKind::UnsupportedVersion)),

            ("HTTP/1.15\r\n",  Err(ErrorKind::InvalidVersion)),
            (" HTTP/1.1\r\n",  Err(ErrorKind::InvalidVersion)),
            ("HTTP/1.1\r \n",  Err(ErrorKind::InvalidVersion)),
            ("HTTP/1.1\r",     Err(ErrorKind::InvalidVersion)),
            ("HTTP/1.0\r",     Err(ErrorKind::InvalidVersion)),
            ("HTTP/1.\n",      Err(ErrorKind::InvalidVersion)),
            ("HTTP/1.1 ",      Err(ErrorKind::InvalidVersion)),
            ("HTTP/1.1",       Err(ErrorKind::InvalidVersion)),
            ("HTTP/1.",        Err(ErrorKind::InvalidVersion)),
            ("\r\n",           Err(ErrorKind::InvalidVersion)),
            ("\r",             Err(ErrorKind::InvalidVersion)),
            ("\n",             Err(ErrorKind::InvalidVersion)),
            (" ",              Err(ErrorKind::InvalidVersion)),
            ("",               Err(ErrorKind::InvalidVersion)),
        ];

        for (value, expected) in cases {
            let mut t = HttpConnection::from_req(value);

            if let Ok((version, has_crlf)) = expected {
                assert_eq!(t.check_version(), Ok(()));

                assert_eq!(t.request.version, version);
                assert_eq!(t.response.version, version);
                assert_eq!(t.parser.has_crlf, has_crlf);

                match t.request.version {
                    Version::Http11 => assert!(t.response.keep_alive),
                    Version::Http10 => assert!(!t.response.keep_alive),
                    Version::Http09 => assert!(!t.response.keep_alive),
                }
            } else if let Err(e) = expected {
                assert_eq!(t.check_version(), Err(e));
            }
        }
    }

    #[test]
    fn parse_header() {
        #[rustfmt::skip]
        let cases = [
            (true,  "HEADER: value\r\n", Some(("header", "value"))),
            (true,  "Header: value\r\n", Some(("header", "value"))),
            (true,  "header: value\r\n", Some(("header", "value"))),
            (true,  "header: \r\n",      Some(("header", ""))),

            (false, "HEADER: value\n",   Some(("header", "value"))),
            (false, "Header: value\n",   Some(("header", "value"))),
            (false, "header: value\n",   Some(("header", "value"))),
            (false, "header: \n",        Some(("header", ""))),
            (true,  "Header : v\r\n",    Some(("header ", "v"))),
            (false, "Header : v\n",      Some(("header ", "v"))),

            (true,  "Header: value\r",   None),
            (false, "Header: value\r",   None),
            (true,  "Header: value",     None),
            (false, "Header: value",     None),
            (true,  "header:value\n",    None),
            (false, "header:value\n",    None),
            (true,  "header:\n",         None),
            (false, "header:\n",         None),
            (true,  ": value\r\n",       None),
            (false, ": value\r\n",       None),
            (true,  ": \r\n",            None),
            (false, ": \r\n",            None),
            (true,  ": value\n",         None),
            (false, ": value\n",         None),
            (true,  ": \n",              None),
            (false, ": \n",              None),

            (false, "HEADER: value\r\n", Some(("header", "value\r"))),
            (false, "Header: value\r\n", Some(("header", "value\r"))),
            (false, "header: value\r\n", Some(("header", "value\r"))),
            (false, "header: \r\n",      Some(("header", "\r"))),
        ];

        for (has_crlf, header, expected) in cases {
            let mut t = HttpConnection::from_req(header);
            t.parser.has_crlf = has_crlf;

            if let Some((name, value)) = expected {
                let header = t.parse_header().unwrap().unwrap();

                assert_eq!(str_op(header.name), name);
                assert_eq!(str_op(header.value), value);
            } else {
                assert_eq!(t.parse_header(), Err(ErrorKind::InvalidHeader));
            }
        }
    }

    #[test]
    fn parse_headers() {
        #[rustfmt::skip]
        let cases = [
            (
                true, "HEADER: value\r\n\r\n",
                Some((vec!["HEADER", "HeAdEr", "header"], "value")),
            ),
            (
                false, "HEADER: value\n\n",
                Some((vec!["HEADER", "HeAdEr", "header"], "value")),
            ),
            (
                true, "HEADER: value\r\nQwE: value\r\nasd: value\r\n\r\n",
                Some((vec!["header", "qwe", "asd"], "value")),
            ),
            (
                false, "HEADER: value\nQwE: value\nasd: value\n\n",
                Some((vec!["header", "qwe", "asd"], "value")),
            ),
            (
                true, "Empty-Value: \r\n\r\n",
                Some((vec!["empty-value"], "")),
            ),
            (
                true, "Space-Value:   \r\n\r\n",
                Some((vec!["space-value"], "  ")),
            ),
            (
                true, "Multi: value1\r\nMulti: value2\r\n\r\n",
                Some((vec!["multi"], "value1")),
            ),

            (true, ": empty-name\r\n\r\n", None),
            (true, "No-Colon value\r\n\r\n", None),
            (
                true, "Valid: ok\r\nInvalidname\r\nNext: value\r\n\r\n",
                None,
            ),
            (true, "Header: value\n\n", None),
            (true, "No-Colon value\r\n\r\n", None),
            (
                true, "Valid: ok\r\nInvalidname\r\nNext: value\r\n\r\n",
                None,
            ),
        ];

        for (has_crlf, headers, expected) in cases {
            let mut t = HttpConnection::from_req(headers);
            t.parser.has_crlf = has_crlf;

            if let Some((names, value)) = expected {
                assert_eq!(t.parse_headers(), Ok(()));

                for name in names {
                    assert_eq!(str(t.request.header(name.as_bytes())), Some(value));
                }
            } else {
                assert_eq!(t.parse_headers(), Err(ErrorKind::InvalidHeader));
            }
        }
    }

    #[test]
    fn parse_special_header() {
        #[rustfmt::skip]
        let cases = [
            ("content-length: 1256\n\n", Ok((Some(1256), None))),
            ("content-length: 4096\n\n", Ok((Some(4096), None))),
            (
                "content-length: 1256\nconnection: keep-alive\n\n", 
                Ok((Some(1256), Some(true)))
            ),
            ("connection: keep-alive\n\n", Ok((None, Some(true)))),
            (
                "content-length: 1256\nconnection: close\n\n", 
                Ok((Some(1256), Some(false)))
            ),
            ("connection: close\n\n", Ok((None, Some(false)))),


            ("connection: keep_alive\n\n", Err(ErrorKind::InvalidConnection)),
            ("connection: qwerrew\n\n", Err(ErrorKind::InvalidConnection)),
            ("content-length: 12asd\n\n", Err(ErrorKind::InvalidContentLength)),
            ("content-length: 123u64\n\n", Err(ErrorKind::InvalidContentLength)),
            ("content-length: 4097\n\n", Err(ErrorKind::BodyTooLarge)),
            ("content-length: 123.9435\n\n", Err(ErrorKind::InvalidContentLength)),
            (
                "content-length: 999999999999999999999\n\n", 
                Err(ErrorKind::InvalidContentLength)
            ),
        ];

        for (headers, result) in cases {
            let mut t = HttpConnection::from_req(headers);

            if let Ok((content_length, keep_alive)) = result {
                assert_eq!(t.parse_headers(), Ok(()));
                assert!(t.request.headers.headers.is_empty());

                if let Some(len) = content_length {
                    assert_eq!(t.request.headers.content_length, Some(len));
                }
                if let Some(keep_alive) = keep_alive {
                    assert_eq!(t.response.keep_alive, keep_alive);
                }
            } else if let Err(e) = result {
                assert_eq!(t.parse_headers(), Err(e));
            }
        }
    }

    macro_rules! parse_request {
        ($cases:expr) => {
            for (req, result) in $cases {
                let mut t = HttpConnection::from_req(req);

                if let Ok(result) = result {
                    assert_eq!(t.parse(), Ok(result.2));

                    assert_eq!(t.request.method(), result.0);
                    assert_eq!(str_op(t.request.url().target()), result.1);
                    assert_eq!(t.response.version, result.2);

                    for (name, value) in result.3 {
                        assert_eq!(
                            str(t.request.header(name.as_bytes())),
                            Some(value.to_string()).as_deref()
                        );
                    }
                    assert_eq!(t.request.body(), result.4);
                    assert_eq!(t.response.keep_alive, result.5);
                } else if let Err(e) = result {
                    assert_eq!(t.parse(), Err(e));
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
            (
                "GET / HTTP/1.1\n\n",
                Ok((
                    Method::Get, "/", Version::Http11, 
                    vec![], 
                    None, true
                )),
            ),
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
                "GET /file HTTP/1.1\ncontent-length: 12\n\nHello world!",
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
                Err(ErrorKind::InvalidMethod)
            ),
            (
                "GET ", 
                Err(ErrorKind::InvalidUrl)
            ),
            (
                "GET  HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n", 
                Err(ErrorKind::InvalidUrl)
            ),
            (
                "GET /\r\n", // Without HTTTP/0.9+ limits
                Err(ErrorKind::InvalidUrl)
            ),
            (
                "GET /HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n", 
                Err(ErrorKind::InvalidVersion)
            ),
            (
                "GET / HTTP/1.1 \r\nHost: 127.0.0.1\r\n\r\n", 
                Err(ErrorKind::InvalidVersion)
            ),
            (
                "GET /HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n", 
                Err(ErrorKind::InvalidVersion)
            ),
            (
                "GET / HTTP/1.1\r\nHost: 127.0.0.1\n\r\n", 
                Err(ErrorKind::InvalidHeader)
            ),
            (
                "GET / HTTP/1.1\nHost: 127.0.0.1\r\n\r\n", 
                Err(ErrorKind::InvalidHeader)
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
    fn parse_http09() {
        use crate::limits::Http09Limits;

        #[rustfmt::skip]
        let cases = vec![
            (
                "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                Ok((Version::Http11, "/", true)),
            ),
            (
                "GET /qwe HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                Ok((Version::Http11, "/qwe", true)),
            ),

            (
                "GET /qwe\r\n", Ok((Version::Http09, "/qwe", false))
            ),
            (
                "GET /q/w/r\r\n", Ok((Version::Http09, "/q/w/r", false))
            ),
            (
                "GET /q/w/r/\r\n", Ok((Version::Http09, "/q/w/r/", false))
            ),
            (
                "GET /\r\n", Ok((Version::Http09, "/", false))
            ),
            (
                "GET /keep_alive/\r\n", Ok((Version::Http09, "/", true))
            ),
            (
                "GET /keep_alive/url\r\n", Ok((Version::Http09, "/url", true))
            ),
            (
                "GET /keep_alive//double/slash\r\n", 
                Ok((Version::Http09, "//double/slash", true))
            ),
            (
                "GET /keep_alive/%20encoded\r\n", 
                Ok((Version::Http09, "/%20encoded", true))
            ),
            (
                "GET /path?query=1\r\n",
                Ok((Version::Http09, "/path?query=1", false))
            ),
            (
                "GET /keep_alive/path?query=1&q=2\r\n", 
                Ok((Version::Http09, "/path?query=1&q=2", true))
            ),
            (
                "GET /?query\r\n",
                Ok((Version::Http09, "/?query", false))
            ),

            (
                "GET \r\n",  Err(ErrorKind::InvalidUrl)
            ),
            (
                "POST /path\r\n", Err(ErrorKind::InvalidUrl)
            ),
            (
                "GET /keep_alive\r\n",  Err(ErrorKind::InvalidUrl)
            ),
            (
                "GET /keep_alive\r\npath\r\n", Err(ErrorKind::InvalidUrl)
            ),
        ];

        for (req, result) in cases {
            let mut t = HttpConnection::from_req(req);
            t.http_09_limits = Some(Http09Limits::default());

            if let Ok(result) = result {
                assert_eq!(t.parse(), Ok(result.0));

                assert_eq!(t.request.method(), Method::Get);
                assert_eq!(str_op(t.request.url().target()), result.1);
                assert_eq!(t.response.version, result.0);
                assert_eq!(t.response.keep_alive, result.2);
            } else if let Err(e) = result {
                assert_eq!(t.parse(), Err(e));
            }
        }
    }

    #[test]
    fn check_limits() {
        use crate::query::Error as Qerror;

        let limits = ReqLimits::default().precalculate();

        let def_url = "/".to_string();
        let url_size = format!("/{}", "q".repeat(limits.url_size - 1));
        let url_parts = "/q".repeat(limits.url_parts + 1);
        let url_query_parts = format!("/?{}", vec!["q=w"; limits.url_query_parts].join("&"));

        let h_name = "N".repeat(limits.header_name_size);
        let h_value = "v".repeat(limits.header_value_size);

        let body = "b".repeat(limits.body_size);

        #[rustfmt::skip]
        let cases = vec![
            (
                format!("GET {} HTTP/1.1\n\n", url_size),
                Ok((
                    Method::Get, &url_size, Version::Http11, 
                    vec![],
                    None, true
                )),
            ),
            (
                format!("GET {url_size}e HTTP/1.1\n\n"),
                Err(ErrorKind::InvalidUrl),
            ),
            (
                format!("GET {} HTTP/1.1\r\n\r\n",  url_parts),
                Ok((
                    Method::Get, &url_parts, Version::Http11, 
                    vec![],
                    None, true
                )),
            ),
            (
                format!("GET {url_parts}/e HTTP/1.1\r\n\r\n"),
                Err(ErrorKind::InvalidUrl),
            ),
            (
                format!("GET {} HTTP/1.1\n\n", url_query_parts),
                Ok((
                    Method::Get, &url_query_parts, Version::Http11, 
                    vec![],
                    None, true
                )),
            ),
            (
                format!(
                    "GET /?{} HTTP/1.1\n\n", 
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
                format!("GET / HTTP/1.1\r\n{h_name}e: value\r\n\n"),
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
                    "GET / HTTP/1.1\nContent-Length: {}\n\n{body}", 
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
                    "GET / HTTP/1.1\nContent-Length: {}\n\n{body}e", 
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

#[cfg(test)]
mod parser {
    use super::*;
    use crate::tools::*;

    #[test]
    fn reset() {
        let limits = ReqLimits::default().precalculate();
        let mut parser = Parser::new(&limits);

        parser.position = 5;
        parser.len = 10;
        parser.has_crlf = true;
        parser.buffer[0] = b'X';

        parser.reset();

        assert_eq!(Parser::new(&limits), parser);
    }

    // Search level
    #[test]
    fn find_slice() {
        let limits = ReqLimits::default().precalculate();
        let mut parser = Parser::from(&limits, b"GET / HTTP/1.1");

        let slice = parser.find_slice(10, b' ').unwrap();
        assert_eq!(slice, b"GET");
        assert_eq!(parser.position, 4);

        let slice = parser.find_slice(10, b' ').unwrap();
        assert_eq!(slice, b"/");

        assert_eq!(parser.find_slice(limits.precalc.buffer + 1, b' '), None);
        assert_eq!(parser.find_slice(0, b' '), None);
    }

    #[test]
    fn find_char() {
        let limits = ReqLimits::default().precalculate();
        let parser = Parser::from(&limits, b"hello world\nnext line");

        assert_eq!(parser.find_char(20, b' '), Some(5));
        assert_eq!(parser.find_char(20, b'\n'), Some(11));
        assert_eq!(parser.find_char(5, b'x'), None);
        assert_eq!(parser.find_char(3, b'o'), None);
    }

    // Low level
    #[test]
    fn get_slice() {
        let limits = ReqLimits::default().precalculate();
        let parser = Parser::from(&limits, b"test data here");

        assert_eq!(str(parser.get_slice(0, 4)), Some("test"));
        assert_eq!(str(parser.get_slice(5, 4)), Some("data"));
        assert_eq!(str(parser.get_slice(20, 5)), Some("\0\0\0\0\0"));
        assert_eq!(parser.get_slice(limits.precalc.buffer + 1, 10), None);
    }

    #[test]
    fn get_slice_mut() {
        let limits = ReqLimits::default().precalculate();
        let mut parser = Parser::from(&limits, b"original");

        {
            let slice = parser.get_slice_mut(0, 8).unwrap();
            slice.copy_from_slice(b"modified");
        }

        assert_eq!(parser.get_slice(0, 8), Some(b"modified".as_ref()));
    }

    #[test]
    fn get_slice_static() {
        let limits = ReqLimits::default().precalculate();
        let parser = Parser::from(&limits, b"static data");

        assert_eq!(str(parser.get_slice_static(0, 6)), Some("static"));
        assert_eq!(str(parser.get_slice_static(7, 4)), Some("data"));
        assert_eq!(str(parser.get_slice_static(20, 5)), Some("\0\0\0\0\0"));
        assert_eq!(parser.get_slice_static(limits.precalc.buffer + 1, 10), None);
    }

    #[test]
    fn update_position() {
        let limits = ReqLimits::default().precalculate();
        let mut parser = Parser::from(&limits, b"some data");

        let old_pos = parser.update_position(4);
        assert_eq!(old_pos, 0);
        assert_eq!(parser.position, 5);

        let old_pos = parser.update_position(3);
        assert_eq!(old_pos, 5);
        assert_eq!(parser.position, 9);
    }

    // Unsafe level
    #[test]
    fn into_static() {
        let vec = vec![1, 2, 3];
        let mut vec_mut = vec.clone();

        let vec_static = unsafe { Parser::into_static(&vec_mut) };
        assert_eq!(vec_mut, vec_static);

        vec_mut[0] = 2;
        assert_eq!(vec_mut, vec_static);
    }

    // Other
    #[test]
    fn sequence_operations() {
        let limits = ReqLimits::default().precalculate();
        let mut parser = Parser::from(&limits, b"GET /api/users HTTP/1.1");

        let method = parser.find_slice(10, b' ').unwrap();
        assert_eq!(method, b"GET");

        let path = parser.find_slice(15, b' ').unwrap();
        assert_eq!(path, b"/api/users");

        let version = parser.get_slice_static(parser.position, 8).unwrap();
        assert_eq!(version, b"HTTP/1.1");
    }
}
