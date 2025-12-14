# Performance Benchmarks

Comparison of maker_web with other HTTP servers. Tests are executed using [wrk](https://github.com/wg/wrk). The benchmark suite measures throughput, RPS and latency under varying connection counts and thread configurations.

To execute the benchmarks, run `./all_bench.sh`. Output is written to the `result` directory.

# Plans for the future

I plan to add benchmarks with other libraries, as well as benchmarks of the library code itself, for continuous optimization.