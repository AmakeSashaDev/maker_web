# Changelog

## 0.1.2

### Parser transition from `v1` to `v2` 🎉🎉🎉

### Added

- Support for all methods for `HTTP/0.9+`
- New error for the client:
  - `DoubleSlash` - If there are 2 or more consecutive slashes in the URL
  - `InvalidEncoding` - If the request (not including the body) is not `UTF-8`
- New limits:
  - `ReqLimits::url_query_size` - Maximum query string lengthbefore closing connection
- Enhanced `Request` documentation and new methods:
  - `Input data requirements` section in `Request` struct documentation
  - `Request::server_addr` - server socket address
  - `Request::client_addr` - client socket address
  - `Request::header_str` - case-insensitive header lookup returning `&str`
  - `Request::is_keep_alive` connection keep-alive status
- Utilities with the `_str` prefix for working with parts of the struct `Url`:
  - `Url::target_str()` - get full URL target with query
  - `Url::path_str()` - get only path component
  - `Url::path_segment_str()` - get specific segment by index
  - `Url::path_segments_str()` - get all path segments as `&[&str]`
  - `Url::matches_str()` - check if path matches given segments
  - `Url::starts_with_str()` - check if path starts with segments  
  - `Url::ends_with_str()` - check if path ends with segments
  - `Url::query_full_str()` - get full query string with `?`
  - `Url::query_str()` - get specific query parameter value
- Small methods:
  - `Method::as_str` - Converts `Method` to `&str` in uppercase
  - `Version::as_str` - Converts `Version` to `&str` in standard format

### Changed

- Parser behavior:
  - Added `UTF-8` check for the entire request, except the body (needed for working with `&str`)
  - Removed support for `\n` as a line separator, now the parser only supports `\r\n`
  - Removed handling of consecutive slashes in URLs - the parser now considers this an error
  - Header format requirement: 
    ```
    [NAME]: SP [VALUE] CRLF
    ```
- **Internal**: `Url::target()`, `Url::path()`, and `Url::query_full()` are no longer `const fn`

## 0.1.1

### Added

- `ConnectionFilter` trait for pre-HTTP connection validation
- `Response::close_without_response()` method for silent termination
- `ServerLimits::count_503_handlers` field for queue overflow handling

### Changed

- Simplified `Handler` trait implementation (ConnectionData is now optional):
  ```rust
  // Before:
  impl Handler<()> for MyHandler {}
  
  // After:
  impl Handler for MyHandler {}
  
  // But it still works the same:
  impl Handler<MyConnData> for MyHandler {}
  ```
- Reduced default `ServerLimits` values (**DDoS protection**):
  - max_connections: `1000` -> `100`
  - max_pending_connections: `10_000` -> `250`

### Deleted

- The `http-json-error` feature (**had no implementation**). Was implemented in `ServerLimits::json_errors` since `0.1.0`

## 0.1.0

Initial release with `HTTP/1.X` and `HTTP/0.9+` support.

## September 3, 2025

🎉🎉🎉 Starting to create a library 🎉🎉🎉
