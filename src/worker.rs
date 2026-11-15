//! Background rendering. Only the newest job is kept; results are handed back
//! to the UI thread through a window message.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

use crate::diff::{self, PageState};
use crate::render::Renderer;

#[derive(Debug)]
pub struct Job {
    pub doc: isize,
    pub reset: bool,
    pub text: String,
    pub base: Option<PathBuf>,
    pub math: bool,
}

/// A finished job. `message` is `None` when the page is already up to date.
#[derive(Debug)]
pub struct Output {
    pub doc: isize,
    pub message: Option<String>,
}

#[derive(Debug, Default)]
struct Shared {
    job: Option<Job>,
    out: VecDeque<Output>,
    stop: bool,
}

type Notify = Box<dyn Fn() + Send>;

pub struct Worker {
    shared: Arc<(Mutex<Shared>, Condvar)>,
    handle: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Worker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Worker").finish_non_exhaustive()
    }
}

fn lock(m: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Worker {
    /// Starts the thread. `notify` is called after each finished job.
    pub fn start(notify: Notify) -> Self {
        let shared = Arc::new((Mutex::new(Shared::default()), Condvar::new()));
        let thread_shared = Arc::clone(&shared);
        let handle = thread::Builder::new().name("mahimarkd-render".into()).spawn(move || run(thread_shared, notify)).ok();
        Self { shared, handle }
    }

    pub fn submit(&self, job: Job) {
        let (m, cv) = &*self.shared;
        let mut s = lock(m);
        let reset = job.reset || s.job.as_ref().is_some_and(|j| j.reset);
        s.job = Some(Job { reset, ..job });
        cv.notify_one();
    }

    pub fn take_outputs(&self) -> Vec<Output> {
        let (m, _) = &*self.shared;
        lock(m).out.drain(..).collect()
    }

    pub fn stop(&mut self) {
        {
            let (m, cv) = &*self.shared;
            lock(m).stop = true;
            cv.notify_all();
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(shared: Arc<(Mutex<Shared>, Condvar)>, notify: Notify) {
    let (m, cv) = &*shared;
    let mut page: Option<(isize, PageState)> = None;
    let mut renderer = Renderer::new(None);
    loop {
        let job = {
            let mut s = lock(m);
            loop {
                if s.stop {
                    return;
                }
                if let Some(job) = s.job.take() {
                    break job;
                }
                s = cv.wait(s).unwrap_or_else(PoisonError::into_inner);
            }
        };

        if renderer.base() != job.base.as_ref() {
            renderer = Renderer::new(job.base.clone());
            page = None;
        }
        let rendered = renderer.render(&job.text, job.math);
        let prev = match &page {
            Some((doc, state)) if *doc == job.doc && !job.reset => Some(state),
            _ => None,
        };
        let (message, state) = diff::build(prev, &rendered);
        page = Some((job.doc, state));
        lock(m).out.push_back(Output { doc: job.doc, message });
        notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn renders_full_then_patch() {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let mut w = Worker::start(Box::new(move || {
            let _ = tx.lock().map(|t| t.send(()));
        }));
        w.submit(Job { doc: 1, reset: false, text: "a\n\nb\n".into(), base: None, math: true });
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let message = |o: &Output| o.message.clone().unwrap_or_default();
        let out = w.take_outputs();
        assert!(message(&out[0]).starts_with("{\"t\":\"full\""));

        w.submit(Job { doc: 1, reset: false, text: "a\n\nc\n".into(), base: None, math: true });
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(message(&w.take_outputs()[0]).starts_with("{\"t\":\"patch\""));

        w.submit(Job { doc: 1, reset: false, text: "a\n\nc\n".into(), base: None, math: true });
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(w.take_outputs()[0].message.is_none());

        w.submit(Job { doc: 2, reset: false, text: "a\n".into(), base: None, math: true });
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let out = w.take_outputs();
        assert_eq!(out[0].doc, 2);
        assert!(message(&out[0]).starts_with("{\"t\":\"full\""));
        w.stop();
    }
}
