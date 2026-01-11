# Request Inspector

A debug HTTP server that shows detailed information about incoming requests. Returns method, path, headers (if present), and body as JSON

**Example Features:**
- Shows HTTP method, URL path, and request body
- Conditionally includes headers (only if they exist)
- UTF-8 safe string handling
- Clean JSON output without empty fields

## Launch
```
cargo run --example request_inspector
```

## Usage
- Basic GET request
  ```
  curl http://localhost:8080/api/users
  # {"method": "GET", "path": "/api/users", "user_agent": "curl/8.16.0"}
  ```
- GET with User-Agent header
  ```
  curl -H "User-Agent: MyApp/1.0" http://localhost:8080/test
  # {"method": "GET", "path": "/test", "user_agent": "MyApp/1.0"}
  ```
- POST with JSON body and headers
  ```
  curl -X POST http://localhost:8080/data -H "Content-Type: application/json" -d "{\"name\": \"John\", \"age\": 30}"
  {"method": "POST", "path": "/data", "user_agent": "curl/8.16.0", "content_type": "application/json", "body": "{\"name\": \"John\", \"age\": 30}"}
  ```