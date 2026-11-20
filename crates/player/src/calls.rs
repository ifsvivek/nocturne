//! The call worker behind `EmbeddedPlayer`: every libmpv call from the UI thread runs here, so a core
//! thread that blocks (a PipeWire shutdown that never returns) cannot freeze the UI.
//!
//! Policy: a read fails at once while anything is running or queued (reads never pile up); a write is
//! always queued, in order, and the caller waits for its result only when the worker was idle,
//! otherwise it gets `Ok` at once (the command runs when the core frees, mpv's events show the effect).
//! The queue is bounded: past `QUEUE_MAX` the oldest seek or volume write is dropped, never a
//! pause, stop or load. Pure of libmpv, so the policy is unit-tested with fake jobs.
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub(crate) const QUEUE_MAX: usize = 64;

/// What a queued write is, for the bound: only `Droppable` ones are ever discarded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Droppable,
    Keep,
}

type Job = Box<dyn FnOnce() + Send>;

#[derive(Default)]
pub(crate) struct Queue {
    jobs: VecDeque<(Kind, Job)>,
    running: Option<Instant>,
    closed: bool,
}

impl Queue {
    fn busy(&self) -> bool {
        self.running.is_some() || !self.jobs.is_empty()
    }

    /// Adds a job, first making room: past the bound the oldest droppable one goes (or the new job
    /// itself when it is droppable and nothing older is).
    pub(crate) fn push(&mut self, kind: Kind, job: Job) {
        if self.jobs.len() >= QUEUE_MAX {
            if let Some(i) = self.jobs.iter().position(|(k, _)| *k == Kind::Droppable) {
                self.jobs.remove(i);
            } else if kind == Kind::Droppable {
                return;
            }
        }
        self.jobs.push_back((kind, job));
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.jobs.len()
    }
}

struct Inner {
    q: Mutex<Queue>,
    cv: Condvar,
}

/// Outcome of submitting a write.
pub(crate) enum Written<T> {
    /// It ran (or the wait ended with its result).
    Done(T),
    /// The worker was busy or did not answer in time; the write stays queued.
    Queued,
}

#[derive(Clone)]
pub(crate) struct Runner(Arc<Inner>);

fn lock(q: &Mutex<Queue>) -> MutexGuard<'_, Queue> {
    q.lock().unwrap_or_else(|p| p.into_inner())
}

impl Runner {
    /// `stall`: debug QA hook, the worker parks for good from that instant on (a simulated stuck core).
    pub(crate) fn new(stall: Option<Instant>) -> Self {
        let inner = Arc::new(Inner {
            q: Mutex::new(Queue::default()),
            cv: Condvar::new(),
        });
        let i2 = inner.clone();
        let _ = std::thread::Builder::new()
            .name("video-calls".into())
            .spawn(move || {
                loop {
                    let job = {
                        let mut q = lock(&i2.q);
                        loop {
                            if q.closed {
                                return;
                            }
                            if let Some((_, j)) = q.jobs.pop_front() {
                                q.running = Some(Instant::now());
                                break j;
                            }
                            q = i2.cv.wait(q).unwrap_or_else(|p| p.into_inner());
                        }
                    };
                    if stall.is_some_and(|t| Instant::now() >= t) {
                        loop {
                            std::thread::park();
                        }
                    }
                    job();
                    lock(&i2.q).running = None;
                }
            });
        Self(inner)
    }

    /// Start of the job running now, `None` when idle: the stuck-core watch reads it.
    pub(crate) fn running_since(&self) -> Option<Instant> {
        lock(&self.0.q).running
    }

    /// Drops everything still queued and ends the worker after its current job.
    pub(crate) fn close(&self) {
        let mut q = lock(&self.0.q);
        q.closed = true;
        q.jobs.clear();
        self.0.cv.notify_all();
    }

    fn enqueue<T: Send + 'static>(
        &self,
        kind: Kind,
        wait: Duration,
        f: impl FnOnce() -> T + Send + 'static,
    ) -> Written<T> {
        let (tx, rx): (_, Receiver<T>) = channel();
        let idle = {
            let mut q = lock(&self.0.q);
            let idle = !q.busy();
            if q.closed {
                return Written::Queued;
            }
            q.push(
                kind,
                Box::new(move || {
                    let _ = tx.send(f());
                }),
            );
            self.0.cv.notify_one();
            idle
        };
        if !idle {
            return Written::Queued;
        }
        rx.recv_timeout(wait).map_or(Written::Queued, Written::Done)
    }

    /// A write: always queued in order; see the module docs.
    pub(crate) fn write<T: Send + 'static>(
        &self,
        kind: Kind,
        wait: Duration,
        f: impl FnOnce() -> T + Send + 'static,
    ) -> Written<T> {
        self.enqueue(kind, wait, f)
    }

    /// A read: `None` at once while anything is running or queued, else waits up to `wait`.
    pub(crate) fn read<T: Send + 'static>(
        &self,
        wait: Duration,
        f: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        if lock(&self.0.q).busy() {
            return None;
        }
        match self.enqueue(Kind::Keep, wait, f) {
            Written::Done(v) => Some(v),
            Written::Queued => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::sync_channel;

    const W: Duration = Duration::from_millis(100);

    #[test]
    fn writes_queue_in_order_behind_a_slow_call_and_reads_fail_fast() {
        let r = Runner::new(None);
        let (gate_tx, gate_rx) = sync_channel::<()>(0);
        let log = Arc::new(Mutex::new(vec![]));
        let l = log.clone();
        // The slow call: the caller gives up waiting, the job stays running.
        assert!(matches!(
            r.write(Kind::Keep, W, move || {
                gate_rx.recv().unwrap();
                l.lock().unwrap().push("slow");
            }),
            Written::Queued
        ));
        assert_eq!(
            r.read(W, || 1),
            None,
            "reads never queue behind a slow call"
        );
        for n in ["pause", "seek", "stop"] {
            let l = log.clone();
            assert!(matches!(
                r.write(Kind::Keep, W, move || l.lock().unwrap().push(n)),
                Written::Queued
            ));
        }
        assert_eq!(r.read(W, || 1), None, "still busy with writes queued");
        gate_tx.send(()).unwrap();
        let t = Instant::now();
        while r.read(W, || 7) != Some(7) {
            assert!(t.elapsed() < Duration::from_secs(5), "worker never freed");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(*log.lock().unwrap(), ["slow", "pause", "seek", "stop"]);
        assert!(matches!(r.write(Kind::Keep, W, || 5), Written::Done(5)));
        r.close();
    }

    #[test]
    fn the_queue_is_bounded_and_keeps_pause_and_stop() {
        let mut q = Queue::default();
        let noop = || -> Job { Box::new(|| {}) };
        q.push(Kind::Keep, noop());
        for _ in 0..QUEUE_MAX * 2 {
            q.push(Kind::Droppable, noop());
        }
        assert_eq!(q.len(), QUEUE_MAX);
        assert_eq!(
            q.jobs.front().map(|j| j.0),
            Some(Kind::Keep),
            "the oldest Keep stays"
        );
        // Only Keep entries left: a droppable newcomer is discarded, a Keep one still gets in.
        let mut q = Queue::default();
        for _ in 0..QUEUE_MAX {
            q.push(Kind::Keep, noop());
        }
        q.push(Kind::Droppable, noop());
        assert_eq!(q.len(), QUEUE_MAX);
        q.push(Kind::Keep, noop());
        assert_eq!(q.len(), QUEUE_MAX + 1);
    }
}
