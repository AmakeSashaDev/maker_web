# Examples

This directory contains examples of using the maker_web library. Each example is a local function.

## List

### ["Hello, world!"](hello_world.md)
**File:** [`hello_world.rs`](hello_world.rs)

A minimal HTTP server that returns "Hello, world!" for all requests.

---

### [Multilingual Greeting](multilingual_greeting.md)
**File:** [`multilingual_greeting.rs`](multilingual_greeting.rs)

A simple HTTP server that returns "Hello World!" in different languages depending on the first segment of the URL path.

---

### [Request Counter](request_counter.md)
**File:** [`request_counter.rs`](request_counter.rs)

A simple HTTP server that counts connection requests. Each connection has its own counter.

---

### [Echo Service](echo.md)
**File:** [`echo.rs`](echo.rs)

A simple HTTP echo server that returns the url and request body in JSON format.

---

### [Request Inspector](request_inspector.md)
**File:** [`request_inspector.rs`](request_inspector.rs)

A debug HTTP server that shows detailed information about incoming requests. Returns method, path, headers (if present), and body as JSON

---

## What's Next?

Check the [API documentation](https://docs.rs/maker_web/latest/maker_web/) for complete reference.
