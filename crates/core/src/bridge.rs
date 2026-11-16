//! Async bridge decision: `nocturne-jellyfin` is async (reqwest on tokio) while the UI runs its
//! own executor on the main thread. We keep ONE process-wide multi-thread tokio runtime on
//! its own worker threads and hand futures to it with `spawn`. The returned `JoinHandle` is a
//! plain future with no tokio-context requirement, so a UI task (or an Iced `Task::perform`) can `.await` it
//! and then update the UI state on the UI thread. No channels, no `block_on`, no nested runtimes.
use std::future::Future;
use std::sync::OnceLock;
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

static RT: OnceLock<Runtime> = OnceLock::new();

pub fn spawn<T: Send + 'static>(fut: impl Future<Output = T> + Send + 'static) -> JoinHandle<T> {
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("nocturne-io")
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
    .spawn(fut)
}
