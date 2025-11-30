//! Zero-copy URL query string parser with flexible collection support.

use memchr::memchr;
use std::{collections::HashMap, error, fmt};

/// Zero-copy URL query string parser.
///
/// Provides high-performance parsing of URL query strings without allocating
/// new strings for parameter names and values.
///
/// Can be used to parse form data (application/x-www-form-urlencoded),
/// but **there is currently no decoder support (%20, %40, etc.)**,
/// due to zero-copy & zero-alloc.
/// URL decoding may be added in future versions via optional features.
///
/// # Examples
/// ```rust
/// use maker_web::query::Query;
/// use std::collections::HashMap;
///
/// // Parse into Vec (preserves order)
/// let query = b"name=john&age=25&city";
/// let vec_params: Vec<(&[u8], &[u8])> = Query::parse(query, 10).unwrap();
/// assert_eq!(vec_params.len(), 3);
///
/// // Parse into HashMap (deduplicates)
/// let hash_params: HashMap<&[u8], &[u8]> = Query::parse(query, 10).unwrap();
/// assert_eq!(hash_params.len(), 3);
///
/// // Handle limits
/// let result = Query::parse::<Vec<(&[u8], &[u8])>>(b"a=1&b=2", 1);
/// assert!(result.is_err()); // Exceeds limit of 1 parameter
/// ```
/// All possible formats:
/// ```rust
/// use maker_web::query::Query;
///
/// let query = b"debug&name=&=Qwe&key=sda&&";
/// let vec_params: Vec<(&[u8], &[u8])> = Query::parse(query, 10).unwrap();
///
/// assert_eq!(vec_params.len(), 5);
/// assert!(vec_params[0] == (b"debug", b""));
/// assert!(vec_params[1] == (b"name", b""));
/// assert!(vec_params[2] == (b"", b"Qwe"));
/// assert!(vec_params[3] == (b"key", b"sda"));
/// assert!(vec_params[4] == (b"", b""));
/// assert!(vec_params.get(5).is_none());
/// ```
pub struct Query;

impl Query {
    /// Parses a URL query string into a new collection.
    ///
    /// Helper method for parsing query strings into custom collections.
    /// Used internally by [Query::parse_into] for flexible parameter handling.
    ///
    /// # Type Parameters
    /// - `C`: Collection type implementing [QueryCollector]
    ///
    /// # Arguments
    /// - `query`: Raw bytes of the query string
    ///   (handles optional leading `?` automatically, so `?a=1` and `a=1` are equivalent)
    /// - `limit`: Maximum number of parameters to parse
    ///
    /// # Examples
    /// ```
    /// use maker_web::query::Query;
    /// use std::collections::HashMap;
    ///
    /// // Parse into Vec (preserves order)
    /// let params: Vec<(&[u8], &[u8])> = Query::parse(b"name=john&age=25", 10).unwrap();
    /// assert_eq!(params.len(), 2);
    ///
    /// // Parse into HashMap (deduplicates keys)
    /// let params: HashMap<&[u8], &[u8]> = Query::parse(b"key=1&key=2", 10).unwrap();
    /// assert_eq!(params.len(), 1); // only last value remains
    ///
    /// // Handle empty values and missing '='
    /// let params: Vec<(&[u8], &[u8])> = Query::parse(b"flag&empty=", 10).unwrap();
    /// assert!(params[0] == (b"flag", b""));
    /// assert!(params[1] == (b"empty", b""));
    /// ```
    #[inline(always)]
    pub fn parse<'a, C: QueryCollector<'a>>(query: &'a [u8], limit: usize) -> Result<C, Error> {
        let mut result = C::with_capacity(limit);
        Self::parse_into(&mut result, query, limit)?;
        Ok(result)
    }

    /// Parses a URL query string into an existing collection.
    ///
    /// This method allows reusing collection instances and provides more
    /// control over the parsing process.
    ///
    /// # Type Parameters
    /// - `C`: Collection type implementing [QueryCollector]
    ///
    /// # Arguments
    /// - `result`: Mutable reference to existing collection
    /// - `query`: Raw bytes of the query string
    /// - `limit`: Maximum number of parameters to parse
    ///
    /// # Examples
    /// ```
    /// use maker_web::query::Query;
    ///
    /// // Reuse collection for multiple parses
    /// let mut collector = Vec::new();
    ///
    /// Query::parse_into(&mut collector, b"a=1&b=2", 10).unwrap();
    /// assert_eq!(collector.len(), 2);
    ///
    /// Query::parse_into(&mut collector, b"c=3&d=4", 10).unwrap();
    /// assert_eq!(collector.len(), 4); // parameters are appended
    ///
    /// // Handle limits
    /// let mut collector = Vec::new();
    /// let result = Query::parse_into(&mut collector, b"a=1&b=2&c=3", 2);
    /// assert!(result.is_err()); // limit exceeded after 2 parameters
    ///
    /// // Parse form data with URL-encoded values (no decoding)
    /// let mut collector = Vec::new();
    /// Query::parse_into(&mut collector, b"email=user%40example.com", 10).unwrap();
    /// assert_eq!(collector[0].1, b"user%40example.com"); // raw bytes
    /// ```
    #[inline]
    pub fn parse_into<'a, C: QueryCollector<'a>>(
        result: &mut C,
        query: &'a [u8],
        limit: usize,
    ) -> Result<(), Error> {
        let data = match query.first().ok_or(Error::Empty)? {
            b'?' => &query[1..],
            _ => query,
        };

        let mut start = 0;
        while start < data.len() {
            // Check parameter limit
            if result.length() >= limit {
                return Err(Error::OverLimit(limit));
            }

            // Find next '&' or end of string
            let end = memchr(b'&', &data[start..])
                .map(|pos| start + pos)
                .unwrap_or(data.len());

            // Find '=' within current parameter segment
            let index = memchr(b'=', &data[start..end]).unwrap_or(end - start);
            let split_index = start + index;

            // Extract key and value
            let key = &data[start..split_index];
            let value = match split_index < end {
                true => &data[split_index + 1..end], // Has value after '='
                false => b"",                        // No value (key only)
            };

            result.add_param(key, value);
            start = end + 1;
        }

        Ok(())
    }
}

/// A trait for types that can collect parsed query parameters.
///
/// This trait allows flexible storage of URL query parameters while maintaining
/// zero-copy parsing. Implementors can choose how to store the key-value pairs.
///
/// # Lifetime
/// - `'a`: The lifetime of the input query string bytes
///
/// # Examples
/// ```rust
/// use maker_web::query::QueryCollector;
///
/// struct SimpleCollector(Vec<(String, String)>);
///
/// impl<'a> QueryCollector<'a> for SimpleCollector {
///     fn add_param(&mut self, key: &'a [u8], value: &'a [u8]) {
///         self.0.push((
///             String::from_utf8_lossy(key).to_string(),
///             String::from_utf8_lossy(value).to_string(),
///         ));
///     }
///
///     fn length(&self) -> usize {
///         self.0.len()
///     }
///
///     fn with_capacity(capacity: usize) -> Self {
///         SimpleCollector(Vec::with_capacity(capacity))
///     }
/// }
/// ```
pub trait QueryCollector<'a>
where
    Self: Sized,
{
    /// Adds a parsed parameter to the collection.
    ///
    /// # Arguments
    /// - `key`: The parameter name as bytes (empty if no value provided)
    /// - `value`: The parameter value as bytes (empty if no value provided)
    fn add_param(&mut self, key: &'a [u8], value: &'a [u8]);

    /// Returns the current number of parameters in the collection.
    // For `length` instead of `len`, thanks to `clippy` for the tip
    // about adding the `is_empty` method, although it's not needed here
    fn length(&self) -> usize;

    /// Creates a new collection with the specified capacity.
    ///
    /// # Arguments
    /// - `capacity`: The initial capacity for the collection
    fn with_capacity(capacity: usize) -> Self;
}

// Implementation for Vec - preserves parameter order
impl<'a> QueryCollector<'a> for Vec<(&'a [u8], &'a [u8])> {
    #[inline(always)]
    fn add_param(&mut self, key: &'a [u8], value: &'a [u8]) {
        self.push((key, value));
    }

    #[inline(always)]
    fn length(&self) -> usize {
        self.len()
    }

    #[inline(always)]
    fn with_capacity(capacity: usize) -> Self {
        Vec::with_capacity(capacity)
    }
}

// Implementation for HashMap - deduplicates parameters (last wins)
impl<'a> QueryCollector<'a> for HashMap<&'a [u8], &'a [u8]> {
    #[inline(always)]
    fn add_param(&mut self, key: &'a [u8], value: &'a [u8]) {
        self.insert(key, value);
    }

    #[inline(always)]
    fn length(&self) -> usize {
        self.len()
    }

    #[inline(always)]
    fn with_capacity(capacity: usize) -> Self {
        HashMap::with_capacity(capacity)
    }
}

/// Error types that can occur during query parsing.
///
/// This enum provides detailed error information for different failure scenarios
/// when parsing URL query strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The number of parameters exceeded the specified limit.
    ///
    /// This error occurs when the query string contains more parameters
    /// than the allowed maximum specified in the `limit` argument.
    ///
    /// # Fields
    /// - `0`: The maximum allowed number of parameters
    OverLimit(usize),

    /// The query string is empty or contains only a '?' character.
    ///
    /// This error occurs when the input query string has no meaningful content
    /// to parse (empty, or just "?").
    Empty,
}

impl error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::OverLimit(limit) => {
                write!(f, "Query parameter limit exceeded: limit={}", limit)
            }
            Error::Empty => {
                write!(f, "Query string is empty or contains no parameters")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::*;

    #[test]
    fn basic() {
        let cases = ["a=1&b=2", "?a=1&b=2"];

        for line in cases {
            let params: Vec<(&[u8], &[u8])> = Query::parse(line.as_bytes(), 8).unwrap();

            assert_eq!(params.len(), 2);
            assert_eq!(str_2(params[0]), ("a", "1"));
            assert_eq!(str_2(params[1]), ("b", "2"));
        }
    }

    #[test]
    fn full() {
        let line = b"flag&empty=&=val&&key=value";
        let params: Vec<(&[u8], &[u8])> = Query::parse(line, 10).unwrap();

        assert_eq!(params.len(), 5);
        assert_eq!(str_2(params[0]), ("flag", ""));
        assert_eq!(str_2(params[1]), ("empty", ""));
        assert_eq!(str_2(params[2]), ("", "val"));
        assert_eq!(str_2(params[3]), ("", ""));
        assert_eq!(str_2(params[4]), ("key", "value"));
    }

    #[test]
    fn not_complete() {
        let params: Vec<(&[u8], &[u8])> = Query::parse(b"flag&empty=&=val", 10).unwrap();

        assert_eq!(params.len(), 3);
        assert_eq!(str_2(params[0]), ("flag", ""));
        assert_eq!(str_2(params[1]), ("empty", ""));
        assert_eq!(str_2(params[2]), ("", "val"));
    }

    #[test]
    fn limit_error() {
        assert_eq!(
            Query::parse::<Vec<(&[u8], &[u8])>>(b"a&a", 1),
            Err(Error::OverLimit(1))
        );
    }

    #[test]
    fn empty_error() {
        assert_eq!(
            Query::parse::<Vec<(&[u8], &[u8])>>(b"", 10),
            Err(Error::Empty)
        );
    }
}
