# Note

This library currently lacks optimizations such as:

- MIO or lower-level access for IO
- Custom optimized scheduler
- Custom optimized SIMD

**But this is temporary** (but this does not mean that they will be added :) ). The project is evolving, and various optimizations are gradually being added. Thank you for your understanding ❤️

# Performance Benchmarks

Comparison of maker_web with other HTTP servers. Tests are executed using [`wrk`](https://github.com/wg/wrk). The benchmark suite measures throughput, RPS and latency under varying connection counts and thread configurations.

# Launch

Tools required:
* `cargo`
* `rustc`
* `wrk`

Procedure:

1. Open the server folder and run:
   ```bash
   cargo run --release
   ```

2. Run in a separate window:
   ```bash
   chmod +x bench.sh
   ./bench.sh [SERVER_NAME]
   ```
   `[SERVER_NAME]` - server name (affects only the final file name)

3. Wait for script `bench.sh` to complete

# Plans for the future

I plan to add benchmarks with other libraries, as well as benchmarks of the library code itself, for continuous optimization.
