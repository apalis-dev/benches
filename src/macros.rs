/// Macro to define backend benchmarks using Criterion and Tokio.
#[macro_export]
macro_rules! define_backend_bench {
    ($name:expr, $task_count:expr, $setup:expr ) => {
        paste::paste! {
        fn [<$name>](c: &mut Criterion) {
            let mut group = c.benchmark_group($name);
            group.sample_size(10);
            group.bench_with_input(BenchmarkId::new("consume", &$task_count), &$task_count, |b, &s| {
                b.to_async(Runtime::new().unwrap())
                    .iter(|| async move {
                        let storage = { $setup };
                        let mut s1 = storage.clone();
                        tokio::spawn(async move {
                            for _ in 0..s {
                                let _ = s1.push(TestJob).await;
                            }
                        });
                        std::hint::black_box(bench_worker($name, storage.clone(), task_fn(apalis_benches::empty_job), s).await);

                    })
            });
            group.bench_with_input(BenchmarkId::new("push", &$task_count), &$task_count, |b, &s| {
                b.to_async(Runtime::new().unwrap()).iter(|| async move {
                    let mut storage = { $setup };
                    for _i in 0..s {
                        let _ = black_box(storage.push(TestJob).await);
                    }
                });
            });
        }}
    };
}
