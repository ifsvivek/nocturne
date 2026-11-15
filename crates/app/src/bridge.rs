//! Async bridge decision: `nocturne-jellyfin` is async (reqwest on tokio) while GPUI runs its
//! own executor on the main thread. We keep ONE process-wide multi-thread tokio runtime on
//! its own worker threads and hand futures to it with `spawn`. The returned `JoinHandle` is a
//! plain future with no tokio-context requirement, so a GPUI `cx.spawn` task can `.await` it
//! and then update an entity on the UI thread. No channels, no `block_on`, no nested runtimes.
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

/// Runs `fut` on the bridge, then applies the result to the view `V` on the UI thread.
pub fn run<V: 'static, T: Send + 'static>(
    cx: &mut gpui::Context<V>,
    fut: impl Future<Output = T> + Send + 'static,
    done: impl FnOnce(&mut V, T, &mut gpui::Context<V>) + 'static,
) {
    let h = spawn(fut);
    cx.spawn(async move |this, cx| {
        if let Ok(v) = h.await {
            this.update(cx, |t, cx| {
                done(t, v, cx);
                cx.notify();
            })
            .ok();
        }
    })
    .detach();
}
