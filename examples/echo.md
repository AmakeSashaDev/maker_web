# Echo Service

A simple HTTP echo server that returns the url and request body in JSON format.

**Example Features:**
- Returns URL path and request body in JSON format
- Handles any HTTP method (GET, POST, PUT, etc.)
- UTF-8 safe string handling
- JSON responses with proper escaping

## Launch
```
cargo run --example echo
```

## Usage
- GET request with URL path
  ```
  curl http://localhost:8080/api/test
  # {"url": "/api/test"}
  ```
- POST request with JSON body
  ```
  curl -X POST http://localhost:8080/echo -d "{\"message\": \"test\"}"
  # {"url": "/echo", "body": "{\"message\": \"test\"}"}
  ```
- PUT request with plain text
  ```
  curl -X PUT http://localhost:8080/ -d "Hello, World!"
  # {"url": "/", "body": "Hello, World!"}
  ```