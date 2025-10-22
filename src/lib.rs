use apalis_core::backend::Backend;
use apalis_core::error::BoxDynError;
use apalis_core::layers::Service;
use apalis_core::task::Task;
use apalis_core::worker::builder::WorkerBuilder;
use apalis_core::worker::context::WorkerContext;
use apalis_core::worker::ReadinessService;
use apalis_core::worker::TrackerService;
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use tower::Layer;

mod macros;

/// A simple job struct for benchmarking purposes.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TestJob;

/// An empty job function that does nothing.
pub async fn empty_job(_: TestJob) {}

/// Benchmarks a worker processing tasks from the given backend.
pub async fn bench_worker<B, Ctx, Svc>(name: &str, backend: B, service: Svc, task_count: usize)
where
    B::Stream: Send + Unpin + 'static,
    B: Backend<Context = Ctx, Args = TestJob> + 'static + Send,
    Ctx: Send + Sync + 'static,
    Svc: Send + Sync + Service<Task<TestJob, Ctx, B::IdType>> + 'static + Clone,
    Svc::Future: Send,
    Svc::Error: Into<BoxDynError>+ Send+ Sync + 'static,
    B::Layer: Layer<BenchService<ReadinessService<TrackerService<Svc>>>>,
    <B::Layer as Layer<BenchService<ReadinessService<TrackerService<Svc>>>>>::Service:
        Service<Task<TestJob, Ctx, B::IdType>, Response = ()> + Send,
    <<B::Layer as Layer<BenchService<ReadinessService<TrackerService<Svc>>>>>::Service as Service<Task<TestJob, Ctx, B::IdType>>>::Future:
        Send,
    <<B::Layer as Layer<BenchService<ReadinessService<TrackerService<Svc>>>>>::Service as Service<Task<TestJob, Ctx, B::IdType>>>::Error:
        Into<BoxDynError>+ Send+ Sync + 'static,
        B::Args: Send + 'static,
        B::IdType: Send + 'static,
        B::Error: Into<BoxDynError> + Send + Sync + 'static,
        B::Beat: Unpin + Send + 'static,

{
    let mut ctx = WorkerContext::new::<Svc>(&format!("{}-bench", name));
    WorkerBuilder::new(format!("{}-bench", name))
        .backend(backend)
        .layer(BenchLayer {
            counter: Arc::new(AtomicUsize::new(0)),
            task_count,
            worker: ctx.clone(),
        })
        .build(service)
        .run_with_ctx(&mut ctx)
        .await
        .unwrap();
}

/// A Tower layer that wraps services to count processed tasks and stop the worker when done.
pub struct BenchLayer {
    counter: Arc<AtomicUsize>,
    task_count: usize,
    worker: WorkerContext,
}

impl<S> Layer<S> for BenchLayer {
    type Service = BenchService<S>;

    fn layer(&self, service: S) -> Self::Service {
        BenchService {
            counter: self.counter.clone(),
            max_tasks: self.task_count.clone(),
            service,
            worker: self.worker.clone(),
        }
    }
}

/// A Tower service that counts processed tasks and stops the worker when the maximum is reached.
pub struct BenchService<S> {
    counter: Arc<AtomicUsize>,
    max_tasks: usize,
    service: S,
    worker: WorkerContext,
}

impl<S, Request> Service<Request> for BenchService<S>
where
    S: Service<Request>,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future =
        Pin<Box<dyn futures::Future<Output = Result<S::Response, S::Error>> + std::marker::Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let counter = self.counter.clone();
        let worker = self.worker.clone();
        let max_tasks = self.max_tasks;
        self.service
            .call(request)
            .map(move |res| {
                let val = counter.fetch_add(1, Ordering::Acquire);
                if max_tasks - 1 == val {
                    worker.stop().unwrap();
                }
                res
            })
            .boxed()
    }
}
