# README — Include and Benchmark the Backend

This README explains how to include the backend in your project and run the benchmark suite.

## Overview
- Add the backend crate/library to your project.
- Configure any required services (databases, queues).
- Run the benchmark suite and inspect results.

## Prerequisites
- Rust toolchain (stable) and Cargo installed.
- Any backend services the benchmarks require (e.g., Redis, Postgres, SQLite) running and reachable.
- Optional: Criterion for nicer benchmark reports (added in dev-dependencies).

## Add the backend to your project
Example for a local crate or Git dependency in Cargo.toml:

```toml
[dependencies]
my-backend = { path = "../my-backend" }
# or from Git
# my-backend = { git = "https://github.com/your-org/my-backend.git", tag = "vX.Y.Z" }
```

If the bench repository is separate, add your backend as a dependency of the bench crate or use a workspace with member crates.

## Configure the backend
Use environment variables or a small config file the benches read. Example env variables:

```bash
export BACKEND_URL=postgres://user:pass@localhost/db
export REDIS_URL=redis://127.0.0.1/
```

Document required variables in a `.env.example` file.

## Example usage in code
A minimal example showing how tests/benches create a client:

```rust
let cfg = BackendConfig::from_env();
let client = MyBackend::new(cfg).await?;
```

Place benchmark code under `benches/` using Criterion or the standard Rust bench harness.

## Registering a backend benchmark
If your bench harness exposes a convenience macro to register backend benchmarks, document the macro usage and show concrete examples. Example using a `define_backend_bench!` macro:

```rust
define_backend_bench!("sqlite_in_memory", 10000, {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    let _ = SqliteStorage::setup(&pool).await;
    SqliteStorage::new_with_config(
        pool,
        apalis_sqlite::Config::default()
    )
});
```

- First argument: bench identifier/name.
- Second argument: number of operations (or configurable iterations) the bench will run.
- Third argument: async block that constructs and returns the backend/storage instance used by the benchmark. Use this block to set up in-memory DBs, connection pools, or test fixtures. Ensure any required setup (schema creation, migrations, sample data) runs before returning the constructed backend.

Adapt the closure to your backend type (Postgres, Redis, etc.), and keep setup deterministic to reduce variance in results.

## Run benchmarks
- Using Criterion (recommended):
    1. Ensure dev-dependency: `criterion = "0.xx"`
    2. Run:
         ```bash
         cargo bench
         ```
    3. Results and HTML reports are written to `target/criterion/<bench-name>/report/index.html`.

- Run a single bench:
    ```bash
    cargo bench --bench bench_name
    ```

- To measure with specific config:
    ```bash
    BACKEND_URL=... cargo bench
    ```

## Interpret results
- Criterion produces statistical summaries and HTML reports in `target/criterion`.
- For quick comparisons, use `cargo-benchcmp` or `benchcmp` tools to compare outputs between commits.

## CI integration (GitHub Actions)
Simple CI job to run benches and upload artifacts:

```yaml
name: Bench
on: [push]
jobs:
    bench:
        runs-on: ubuntu-latest
        steps:
            - uses: actions/checkout@v4
            - uses: actions-rs/toolchain@v1
                with:
                    toolchain: stable
                    profile: minimal
            - name: Run benchmarks
                run: |
                    export BACKEND_URL=postgres://...
                    cargo bench --no-run
                    cargo bench
            - name: Upload results
                uses: actions/upload-artifact@v4
                with:
                    name: criterion-reports
                    path: target/criterion
```

## Troubleshooting
- Bench hangs: confirm backend service is reachable and credentials are correct.
- No benchmark output: ensure benches are in `benches/` or use Criterion’s macros correctly.
- For noisy environments, increase Criterion sample size in the bench code.

## Contributing
- Keep the bench harness deterministic.
- Document required external services and sample data setup.
- Add CI checks that run benchmarks periodically or on significant performance PRs.

That's it — add the backend as a dependency, configure env vars for services, register your backend benches (example above), run `cargo bench`, and examine `target/criterion` reports.
