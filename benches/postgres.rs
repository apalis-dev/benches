use apalis_benches::define_backend_bench;
use apalis_benches::{bench_worker, TestJob};
use apalis_core::backend::TaskSink;
use apalis_core::task_fn::task_fn;
use apalis_postgres::PostgresStorage;
use apalis_sql::config::Config;
use criterion::Criterion;
use criterion::{criterion_group, criterion_main, BenchmarkId};
use sqlx::PgPool;
use std::hint::black_box;
use tokio::runtime::Runtime;

define_backend_bench!("postgres_basic", 10000, {
    let pool = PgPool::connect(env!("DATABASE_URL")).await.unwrap();
    let _ = PostgresStorage::setup(&pool).await;
    PostgresStorage::new_with_config(&pool, &Config::new("benches_basic"))
});

// define_backend_bench!("postgres_with_notify", 10000, {
//     let pool = PgPool::connect(env!("DATABASE_URL")).await.unwrap();
//     let _ = PostgresStorage::setup(&pool).await;
//     PostgresStorage::new_with_notify(&pool, &Config::new("benches_with_notify")).await
// });

criterion_group!(benches, postgres_basic);
criterion_main!(benches);
