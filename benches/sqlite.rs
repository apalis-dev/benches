use apalis_benches::define_backend_bench;
use apalis_benches::{bench_worker, TestJob};
use apalis_core::backend::TaskSink;
use apalis_core::task_fn::task_fn;
use apalis_sqlite::{Config, SqlitePool, SqliteStorage};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::hint::black_box;
use tokio::runtime::Runtime;

define_backend_bench!("sqlite_in_file", 10000, {
    let pool = SqlitePool::connect("/tmp/test.db").await.unwrap();
    let _ = SqliteStorage::setup(&pool).await;
    SqliteStorage::new_with_config(&pool, &Config::default())
});

define_backend_bench!("sqlite_in_memory", 10000, {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    let _ = SqliteStorage::setup(&pool).await;
    SqliteStorage::new_with_config(&pool, &Config::default())
});

define_backend_bench!("sqlite_in_memory_with_callback", 10000, {
    let config = Config::default();
    let backend = SqliteStorage::new_with_callback(
        "file:sqlite_in_memory_with_callback?mode=memory&cache=shared",
        &config,
    );
    let _ = SqliteStorage::setup(backend.pool()).await;
    backend
});

criterion_group!(benches, sqlite_in_file, sqlite_in_memory, sqlite_in_memory_with_callback);
criterion_main!(benches);
