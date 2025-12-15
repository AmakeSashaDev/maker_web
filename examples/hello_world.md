# "Hello, world!"

A minimal HTTP server that returns "Hello, world!" for all requests.

**Example Features:**
- Simply returns "Hello, world!" with the header "Content-Type: text/plain" :)

## Launch
```
cargo run --example hello_world
```

## Usage
```
curl http://localhost:8080/
# Hello, world!

curl http://localhost:8080/api/test
# Hello, world!

curl -X POST http://localhost:8080/
# Hello, world!
```