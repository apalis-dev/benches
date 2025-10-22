use apalis_benches::define_backend_bench;
use apalis_core::backend::TaskSink;
use apalis_benches::{bench_worker, TestJob};
use criterion::{criterion_group, criterion_main, BenchmarkId};
use criterion::Criterion;
use tokio::runtime::Runtime;
use apalis_sqlite::SqlitePool;
use apalis_sqlite::SqliteStorage;
 use std::hint::black_box;
 use apalis_core::task_fn::task_fn;

define_backend_bench!("sqlite_in_file", 10000, {
    let pool = SqlitePool::connect("/tmp/test.db").await.unwrap();
    let _ = SqliteStorage::setup(&pool).await;
    SqliteStorage::new_with_config(&pool, &apalis_sqlite::Config::default())
});

define_backend_bench!("sqlite_in_memory", 10000, {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    let _ = SqliteStorage::setup(&pool).await;
    SqliteStorage::new_with_config(&pool, &apalis_sqlite::Config::default())
});

criterion_group!(benches, sqlite_in_file, sqlite_in_memory);
criterion_main!(benches);
