# Changelog

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
  ````
- Reduced default `ServerLimits` values (**DDoS protection**):
  - max_connections: `1000` -> `100`
  - max_pending_connections: `10_000` -> `250`

### Deleted
- The `http-json-error` feature (**had no implementation**). Was implemented in `ServerLimits::json_errors` since `0.1.0`

## 0.1.0

Initial release with `HTTP/1.X` and `HTTP/0.9+` support.

## September 3, 2025

🎉🎉🎉 Starting to create a library 🎉🎉🎉
