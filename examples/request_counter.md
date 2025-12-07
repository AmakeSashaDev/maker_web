# Request Counter

A simple HTTP server that counts connection requests. Each connection has its own counter.

**Example Features:**
- Demonstrates ConnectionData trait for per-connection state
- JSON responses with request count
- Automatic counter reset on new connections

## Launch
```
cargo run --example request_counter
```

## Usage
- A new connection for each request
  ```
  A:\projects>curl http://localhost:8080/
  # {"count_request": 1}
  
  A:\projects>curl http://localhost:8080/
  # {"count_request": 1}
  
  A:\projects>curl http://localhost:8080/
  # {"count_request": 1}
  ```
- One connection for several requests
  ```
  curl http://localhost:8080/ http://localhost:8080/ http://localhost:8080/
  # {"count_request": 1}{"count_request": 2}{"count_request": 3}
  ```