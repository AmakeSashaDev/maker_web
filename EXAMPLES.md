# Examples

This directory contains examples of using the maker_web library. Each example is a local function.

## List

### Multilingual "Hello, world!" 
**File:** `hello_world_multilang.rs`

A simple HTTP server that returns "Hello World!" in different languages depending on the first segment of the URL path.

---

### Request Counter
**File:** `request_counter.rs`

A simple HTTP server that counts connection requests. Each connection has its own counter.

---

### Echo Service
**File:** `echo.rs`

A simple HTTP echo server that returns the url and request body in JSON format.

---

### Request Inspector
**File** `request_inspector.rs`

A debug HTTP server that shows detailed information about incoming requests. Returns method, path, headers (if present), and body as JSON

---

## What's Next?

Check the [API documentation](https://docs.rs/maker_web/latest/maker_web/) for complete reference.