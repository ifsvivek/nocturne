//! The only way UI code starts network work: run the future on the process-wide tokio runtime
//! (`nocturne_core::bridge`) and map its result into a message. A panicked future drops silently.
use iced::Task;
use std::future::Future;
use std::time::Duration;

pub fn run<T, M>(
    fut: impl Future<Output = T> + Send + 'static,
    f: impl FnOnce(T) -> M + Send + 'static,
) -> Task<M>
where
    T: Send + 'static,
    M: Send + 'static,
{
    Task::perform(
        async move { nocturne_core::bridge::spawn(fut).await.ok() },
        move |r| r.map(f),
    )
    .then(|m| match m {
        Some(m) => Task::done(m),
        None => Task::none(),
    })
}

/// `msg` after `d` (timers live on the same runtime).
pub fn after<M: Send + 'static>(d: Duration, msg: M) -> Task<M> {
    run(tokio::time::sleep(d), move |_| msg)
}
