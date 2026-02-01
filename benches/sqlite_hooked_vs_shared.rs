use apalis_core::backend::TaskSink;
use apalis_core::error::BoxDynError;
use apalis_core::task_fn::task_fn;
use apalis_core::worker::builder::WorkerBuilder;
use apalis_core::worker::context::WorkerContext;
use apalis_sqlite::{Config, SharedSqliteStorage, SqliteStorage};
use futures::stream;
use hdrhistogram::Histogram;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};
use tabled::{Table, Tabled};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BenchmarkJob {
    id: usize,
    sleep_ms: u64,
}

#[derive(Debug, Clone, Tabled)]
struct BenchmarkResult {
    #[tabled(rename = "Method")]
    method: String,
    #[tabled(rename = "Jobs")]
    job_count: usize,
    #[tabled(rename = "Workers")]
    worker_count: usize,
    #[tabled(rename = "Job Duration")]
    job_duration_ms: String,
    #[tabled(rename = "Mean (ms)")]
    mean_latency: String,
    #[tabled(rename = "P95 (ms)")]
    p95_latency: String,
    #[tabled(rename = "Throughput (jobs/s)")]
    throughput: String,
    #[tabled(rename = "Peak Mem (MB)")]
    peak_memory_mb: String,
    #[tabled(rename = "Avg Mem (MB)")]
    avg_memory_mb: String,
}

struct BenchmarkMetrics {
    latencies: Mutex<Histogram<u64>>,
    start_time: Instant,
    peak_memory: AtomicU64,
    completed_jobs: AtomicUsize,
    memory_samples: Mutex<Vec<u64>>,
}

impl BenchmarkMetrics {
    fn new() -> Self {
        let latencies = Mutex::new(Histogram::<u64>::new(5).unwrap());
        let start_time = Instant::now();
        let peak_memory = AtomicU64::new(0);
        let completed_jobs = AtomicUsize::new(0);
        let memory_samples = Mutex::new(Vec::new());

        Self {
            latencies,
            start_time,
            peak_memory,
            completed_jobs,
            memory_samples,
        }
    }

    fn record(&self, duration_ms: u64) {
        let _ = self.latencies.lock().unwrap().record(duration_ms);
        self.completed_jobs.fetch_add(1, Ordering::Relaxed);
    }

    fn record_memory(&self, memory_bytes: u64) {
        self.memory_samples.lock().unwrap().push(memory_bytes);

        let mut peak = self.peak_memory.load(Ordering::Relaxed);
        while memory_bytes > peak {
            match self.peak_memory.compare_exchange_weak(
                peak,
                memory_bytes,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(updated_peak) => peak = updated_peak,
            }
        }
    }

    fn generate_result(
        &self,
        method: &str,
        job_count: usize,
        worker_count: usize,
        job_duration_ms: u64,
    ) -> BenchmarkResult {
        let histogram = self.latencies.lock().unwrap();
        let total_time = self.start_time.elapsed().as_secs_f64();
        let throughput_value = self.completed_jobs.load(Ordering::Relaxed) as f64 / total_time;

        let peak_memory = self.peak_memory.load(Ordering::Relaxed);
        let memory_samples = self.memory_samples.lock().unwrap();
        let avg_memory = if memory_samples.is_empty() {
            0
        } else {
            memory_samples.iter().sum::<u64>() / memory_samples.len() as u64
        };

        let method = method.to_string();
        let job_duration_ms = format!("{job_duration_ms}ms");
        let mean_latency = format!("{:.2}", histogram.mean());
        let p95_latency = format!("{:.2}", histogram.value_at_quantile(0.95) as f64);
        let throughput = format!("{:.2}", throughput_value);
        let peak_memory_mb = format!("{:.2}", peak_memory as f64 / 1024.0 / 1024.0);
        let avg_memory_mb = format!("{:.2}", avg_memory as f64 / 1024.0 / 1024.0);

        BenchmarkResult {
            method,
            job_count,
            worker_count,
            job_duration_ms,
            mean_latency,
            p95_latency,
            throughput,
            peak_memory_mb,
            avg_memory_mb,
        }
    }
}

fn build_jobs(job_count: usize, job_duration_ms: u64) -> Vec<BenchmarkJob> {
    (0..job_count)
        .map(|id| BenchmarkJob {
            id,
            sleep_ms: job_duration_ms,
        })
        .collect()
}

async fn monitor_memory(metrics: Arc<BenchmarkMetrics>, job_count: usize) {
    let mut sys = System::new_all();
    let pid = Pid::from(std::process::id() as usize);

    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        sys.refresh_all();

        if let Some(process) = sys.process(pid) {
            let memory = process.memory();
            metrics.record_memory(memory);
        }

        let completed = metrics.completed_jobs.load(Ordering::Relaxed);
        if completed >= job_count {
            break;
        }
    }
}

async fn run_shared_storage_benchmark(
    job_count: usize,
    worker_count: usize,
    job_duration_ms: u64,
) -> Result<BenchmarkResult, BoxDynError> {
    println!(
        "\n[Shared Storage] Running {job_count} jobs with {worker_count} workers, job duration: {job_duration_ms}ms",
    );

    let metrics = Arc::new(BenchmarkMetrics::new());
    let mut store = SharedSqliteStorage::new(":memory:");

    SqliteStorage::setup(store.pool()).await?;

    let storage = store.make_shared()?;
    let mut push_storage = storage.clone();
    let jobs = build_jobs(job_count, job_duration_ms);

    push_storage.push_stream(&mut stream::iter(jobs)).await?;

    let monitor_handle = tokio::spawn(monitor_memory(metrics.clone(), job_count));

    let workers: Vec<_> = (0..worker_count)
        .map(|worker_index| {
            let worker_metrics = metrics.clone();
            let worker_storage = storage.clone();
            tokio::spawn(async move {
                let service = task_fn(move |job: BenchmarkJob, _ctx: WorkerContext| {
                    let metrics = worker_metrics.clone();
                    async move {
                        tokio::time::sleep(Duration::from_millis(job.sleep_ms)).await;
                        metrics.record(metrics.start_time.elapsed().as_millis() as u64);
                        Ok(())
                    }
                });

                WorkerBuilder::new(format!("shared-worker-{worker_index}"))
                    .backend(worker_storage)
                    .build(service)
                    .run()
                    .await
            })
        })
        .collect();

    let timeout = Duration::from_secs(300);
    let check_interval = Duration::from_millis(100);
    let start = Instant::now();

    while metrics.completed_jobs.load(Ordering::Relaxed) < job_count {
        if start.elapsed() > timeout {
            println!("  WARNING: Timeout reached!");
            break;
        }
        tokio::time::sleep(check_interval).await;
    }

    for worker in workers {
        worker.abort();
    }
    monitor_handle.abort();

    tokio::time::sleep(Duration::from_millis(200)).await;

    Ok(metrics.generate_result(
        "Shared (Polling)",
        job_count,
        worker_count,
        job_duration_ms,
    ))
}

async fn run_hooked_storage_benchmark(
    job_count: usize,
    worker_count: usize,
    job_duration_ms: u64,
) -> Result<BenchmarkResult, BoxDynError> {
    println!(
        "\n[Hooked Storage] Running {job_count} jobs with {worker_count} workers, job duration: {job_duration_ms}ms",
    );

    let metrics = Arc::new(BenchmarkMetrics::new());
    let db_path = format!(
        "file:bench_hooked_{}?mode=memory&cache=shared",
        std::process::id(),
    );

    let config = Config::new("bench-queue");
    let backend = SqliteStorage::new_with_callback(&db_path, &config);

    SqliteStorage::setup(backend.pool()).await?;

    let mut push_backend = backend.clone();
    let jobs = build_jobs(job_count, job_duration_ms);

    push_backend.push_stream(&mut stream::iter(jobs)).await?;

    let monitor_handle = tokio::spawn(monitor_memory(metrics.clone(), job_count));

    let workers: Vec<_> = (0..worker_count)
        .map(|worker_index| {
            let worker_metrics = metrics.clone();
            let worker_backend = backend.clone();
            tokio::spawn(async move {
                let service = task_fn(move |job: BenchmarkJob, _ctx: WorkerContext| {
                    let metrics = worker_metrics.clone();
                    async move {
                        tokio::time::sleep(Duration::from_millis(job.sleep_ms)).await;
                        metrics.record(metrics.start_time.elapsed().as_millis() as u64);
                        Ok(())
                    }
                });

                WorkerBuilder::new(format!("hooked-worker-{worker_index}"))
                    .backend(worker_backend)
                    .build(service)
                    .run()
                    .await
            })
        })
        .collect();

    let timeout = Duration::from_secs(300);
    let check_interval = Duration::from_millis(100);
    let start = Instant::now();

    while metrics.completed_jobs.load(Ordering::Relaxed) < job_count {
        if start.elapsed() > timeout {
            println!("  WARNING: Timeout reached!");
            break;
        }
        tokio::time::sleep(check_interval).await;
    }

    for worker in workers {
        worker.abort();
    }
    monitor_handle.abort();

    tokio::time::sleep(Duration::from_millis(200)).await;

    Ok(metrics.generate_result(
        "Hooked (Event-driven)",
        job_count,
        worker_count,
        job_duration_ms,
    ))
}

#[tokio::main]
async fn main() -> Result<(), BoxDynError> {
    let separator = "=".repeat(100);

    println!("{separator}");
    println!("APALIS-SQLITE MEMORY BENCHMARK: Shared (Polling) vs Hooked (Event-driven)");
    println!("{separator}");

    let mut results = Vec::new();

    println!("\n### BENCHMARK 1: Small Workload (100 jobs) ###");
    results.push(run_shared_storage_benchmark(100, 1, 10).await?);
    results.push(run_hooked_storage_benchmark(100, 1, 10).await?);

    println!("\n### BENCHMARK 2: Medium Workload (1000 jobs, varying workers) ###");
    for worker_count in [1, 5, 10] {
        results.push(run_shared_storage_benchmark(1000, worker_count, 10).await?);
        results.push(run_hooked_storage_benchmark(1000, worker_count, 10).await?);
    }

    println!("\n### BENCHMARK 3: Large Burst (5000 jobs) ###");
    results.push(run_shared_storage_benchmark(5000, 10, 5).await?);
    results.push(run_hooked_storage_benchmark(5000, 10, 5).await?);

    println!("\n### BENCHMARK 4: Very Large Burst (10000 jobs) ###");
    results.push(run_shared_storage_benchmark(10000, 10, 5).await?);
    results.push(run_hooked_storage_benchmark(10000, 10, 5).await?);

    println!("\n### BENCHMARK 5: Slower Jobs (500 jobs, 50ms each) ###");
    results.push(run_shared_storage_benchmark(500, 5, 50).await?);
    results.push(run_hooked_storage_benchmark(500, 5, 50).await?);

    println!("\n{separator}");
    println!("FINAL RESULTS");
    println!("{separator}");
    println!("\n{}", Table::new(&results));

    println!("\n{separator}");
    println!("MEMORY USAGE ANALYSIS");
    println!("{separator}");

    let shared_count = results.iter().filter(|r| r.method.contains("Shared")).count() as f64;
    let hooked_count = results.iter().filter(|r| r.method.contains("Hooked")).count() as f64;

    let shared_avg_peak = results
        .iter()
        .filter(|r| r.method.contains("Shared"))
        .filter_map(|r| r.peak_memory_mb.parse::<f64>().ok())
        .sum::<f64>()
        / shared_count;

    let hooked_avg_peak = results
        .iter()
        .filter(|r| r.method.contains("Hooked"))
        .filter_map(|r| r.peak_memory_mb.parse::<f64>().ok())
        .sum::<f64>()
        / hooked_count;

    let shared_avg_mem = results
        .iter()
        .filter(|r| r.method.contains("Shared"))
        .filter_map(|r| r.avg_memory_mb.parse::<f64>().ok())
        .sum::<f64>()
        / shared_count;

    let hooked_avg_mem = results
        .iter()
        .filter(|r| r.method.contains("Hooked"))
        .filter_map(|r| r.avg_memory_mb.parse::<f64>().ok())
        .sum::<f64>()
        / hooked_count;

    println!("\nPeak Memory Usage:");
    println!("  Shared (Polling):      {shared_avg_peak:.2} MB");
    println!("  Hooked (Event-driven): {hooked_avg_peak:.2} MB");

    let peak_diff = ((hooked_avg_peak - shared_avg_peak).abs() / shared_avg_peak) * 100.0;
    let peak_lower = if hooked_avg_peak < shared_avg_peak {
        "Hooked"
    } else {
        "Shared"
    };

    println!(
        "  Difference:            {:.2} MB ({:.1}% lower for {})",
        (hooked_avg_peak - shared_avg_peak).abs(),
        peak_diff,
        peak_lower
    );

    println!("\nAverage Memory Usage:");
    println!("  Shared (Polling):      {shared_avg_mem:.2} MB");
    println!("  Hooked (Event-driven): {hooked_avg_mem:.2} MB");

    let avg_diff = ((hooked_avg_mem - shared_avg_mem).abs() / shared_avg_mem) * 100.0;
    let avg_lower = if hooked_avg_mem < shared_avg_mem {
        "Hooked"
    } else {
        "Shared"
    };

    println!(
        "  Difference:            {:.2} MB ({:.1}% lower for {})",
        (hooked_avg_mem - shared_avg_mem).abs(),
        avg_diff,
        avg_lower
    );

    println!("\nKey Findings:");
    if hooked_avg_peak > shared_avg_peak * 1.1 {
        println!(
            "  NOTE: Hooked storage uses significantly more memory ({:.0}% higher)",
            (hooked_avg_peak / shared_avg_peak - 1.0) * 100.0
        );
    } else if shared_avg_peak > hooked_avg_peak * 1.1 {
        println!(
            "  NOTE: Hooked storage uses less memory ({:.0}% lower)",
            (1.0 - hooked_avg_peak / shared_avg_peak) * 100.0
        );
    } else {
        println!("  NOTE: Both methods have similar memory footprints (< 10% difference)");
    }

    println!("\nMemory Efficiency:");
    println!("  - Shared storage works with simple :memory: mode");
    println!("  - Hooked storage requires shared memory mode with update hooks");
    println!("  - Both scale with job count and worker count");

    Ok(())
}
