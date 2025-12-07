# Examples

This directory contains examples of using the maker_web library. Each example is a local function.

## List

### [Multilingual Greeting](examples/multilingual_greeting.md)
**File:** [`multilingual_greeting.rs`](examples/multilingual_greeting.rs)

A simple HTTP server that returns "Hello World!" in different languages depending on the first segment of the URL path.

---

### [Request Counter](examples/request_counter.md)
**File:** [`request_counter.rs`](examples/request_counter.rs)

A simple HTTP server that counts connection requests. Each connection has its own counter.

---

### [Echo Service](examples/echo.md)
**File:** [`echo.rs`](examples/echo.rs)

A simple HTTP echo server that returns the url and request body in JSON format.

---

### [Request Inspector](examples/request_inspector.md)
**File:** [`request_inspector.rs`](examples/request_inspector.rs)

A debug HTTP server that shows detailed information about incoming requests. Returns method, path, headers (if present), and body as JSON

---

## What's Next?

Check the [API documentation](https://docs.rs/maker_web/latest/maker_web/) for complete reference.