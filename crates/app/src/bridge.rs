//! GPUI side of the async bridge; the runtime itself lives in `nocturne_core::bridge`.
use std::future::Future;

pub use nocturne_core::bridge::spawn;

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
