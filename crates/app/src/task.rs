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
    // On Iced's own executor (not the bridge's): the timer dies with the app instead of being
    // left sleeping on a runtime that is being torn down at exit.
    Task::perform(tokio::time::sleep(d), move |_| msg)
}

/// Hands a value that is not `Clone` (a built `Slot`, the loaded catalog) through a `Message`:
/// the first `take` gets it.
pub struct Handoff<T>(std::sync::Arc<std::sync::Mutex<Option<T>>>);

impl<T> Handoff<T> {
    pub fn new(v: T) -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(Some(v))))
    }

    pub fn take(&self) -> Option<T> {
        self.0.lock().ok()?.take()
    }
}

impl<T> Clone for Handoff<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
