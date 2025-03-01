use apalis_benches::*;
use apalis_core::service_fn::service_fn;
use apalis_core::storage::Storage;
use apalis_redis::RedisStorage;
use apalis_sql::{
    mysql::MysqlStorage,
    postgres::PostgresStorage,
    sqlite::SqliteStorage,
    sqlx::{MySqlPool, PgPool, SqlitePool},
};
use criterion::*;
use paste::paste;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
macro_rules! define_storage_bench {
    ($name:expr, $task_count:expr, $setup:expr ) => {
        paste! {
        fn [<$name>](c: &mut Criterion) {
            let mut group = c.benchmark_group($name);
            group.sample_size(10);
            group.bench_with_input(BenchmarkId::new("consume", &$task_count), &$task_count, |b, &s| {
                b.to_async(Runtime::new().unwrap())
                    .iter_custom(|iters| async move {
                        let storage = { $setup };
                        let mut s1 = storage.clone();
                        for _ in 0..iters {
                            for _i in 0..s {
                                let _ = s1.push(TestJob).await;
                            }
                        }

                        let start = Instant::now();
                        for _ in 0..iters {
                            black_box(bench_worker($name, storage.clone(), service_fn(empty_job), s).await);
                        }
                        let elapsed = start.elapsed();
                        s1.vacuum().await;
                        elapsed
                    })
            });
            group.bench_with_input(BenchmarkId::new("push", &$task_count), &$task_count, |b, &s| {
                b.to_async(Runtime::new().unwrap()).iter(|| async move {
                    let mut storage = { $setup };
                    let start = Instant::now();
                    for _i in 0..s {
                        let _ = black_box(storage.push(TestJob).await);
                    }
                    start.elapsed()
                });
            });
        }}
    };
}

define_storage_bench!("sqlite_in_memory", 10000, {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    let _ = SqliteStorage::setup(&pool).await;
    SqliteStorage::new_with_config(
        pool,
        apalis_sql::Config::default()
            .set_buffer_size(1000)
            .set_poll_interval(Duration::from_millis(100)),
    )
});

define_storage_bench!("redis", 10000, {
    let conn = apalis_redis::connect(env!("REDIS_URL")).await.unwrap();
    let redis = RedisStorage::new_with_config(
        conn,
        apalis_redis::Config::default()
            .set_buffer_size(1000)
            .set_poll_interval(Duration::from_millis(100)),
    );
    redis
});

define_storage_bench!("postgres", 10000, {
    let pool = PgPool::connect(env!("POSTGRES_URL")).await.unwrap();
    let _ = PostgresStorage::setup(&pool).await.unwrap();
    PostgresStorage::new_with_config(
        pool,
        apalis_sql::Config::default()
            .set_buffer_size(1000)
            .set_poll_interval(Duration::from_millis(100)),
    )
});

define_storage_bench!("mysql", 10000, {
    let pool = MySqlPool::connect(env!("MYSQL_URL")).await.unwrap();
    let _ = MysqlStorage::setup(&pool).await.unwrap();
    MysqlStorage::new_with_config(
        pool,
        apalis_sql::Config::default()
            .set_buffer_size(1000)
            .set_poll_interval(Duration::from_millis(100)),
    )
});

criterion_group!(benches, sqlite_in_memory, redis, postgres, mysql);
criterion_main!(benches);
