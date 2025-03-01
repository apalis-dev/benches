use apalis_core::backend::Backend;
use apalis_core::builder::WorkerBuilder;
use apalis_core::builder::WorkerFactory;
use apalis_core::layers::Layer;
use apalis_core::layers::Service;
use apalis_core::notify::Notify;
use apalis_core::request::Request;
use futures::future;
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;

#[derive(Serialize, Deserialize, Debug)]
pub struct TestJob;

pub async fn empty_job(_: TestJob) {}

pub async fn bench_worker<B, Ctx, Svc>(name: &str, backend: B, service: Svc, task_count: usize)
where
    B::Stream: Send + Unpin + 'static,
    B: Backend<Request<TestJob, Ctx>, ()> + 'static,
    Ctx: Send + Sync + 'static,
    Svc: Send + Sync + Service<Request<TestJob, Ctx>, Response = ()> + 'static,
    Svc::Future: Send,
    Svc::Error: Send + Sync + std::error::Error + 'static,
    B::Layer: Layer<BenchService<Svc>>,
    <B::Layer as Layer<BenchService<Svc>>>::Service:
        Service<Request<TestJob, Ctx>, Response = ()> + Send,
    <<B::Layer as Layer<BenchService<Svc>>>::Service as Service<Request<TestJob, Ctx>>>::Future:
        Send,
    <<B::Layer as Layer<BenchService<Svc>>>::Service as Service<Request<TestJob, Ctx>>>::Error:
        Send + Sync + std::error::Error + 'static,
{
    let notify = Notify::new();
    let worker = WorkerBuilder::new(format!("{}-bench", name))
        .layer(BenchLayer {
            counter: Arc::default(),
            sender: notify.clone(),
            task_count,
        })
        .backend(backend)
        .build(service)
        .run();
    let wait_for_completion = notify.notified().boxed();
    future::select(worker, wait_for_completion).await;
}

pub struct BenchLayer {
    counter: Arc<AtomicUsize>,
    sender: Notify<()>,
    task_count: usize,
}

impl<S> Layer<S> for BenchLayer {
    type Service = BenchService<S>;

    fn layer(&self, service: S) -> Self::Service {
        BenchService {
            counter: self.counter.clone(),
            sender: self.sender.clone(),
            max_tasks: self.task_count.clone(),
            service,
        }
    }
}
pub struct BenchService<S> {
    counter: Arc<AtomicUsize>,
    sender: Notify<()>,
    max_tasks: usize,
    service: S,
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
        let sender = self.sender.clone();
        let max_tasks = self.max_tasks;
        self.service
            .call(request)
            .map(move |res| {
                let val = counter.fetch_add(1, Ordering::Relaxed);
                if max_tasks - 1 == val {
                    sender.notify(()).unwrap();
                }
                res
            })
            .boxed()
    }
}
