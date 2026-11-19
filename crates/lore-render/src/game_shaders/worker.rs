//! The thread that compiles game shaders, so a change never stalls a frame.
//!
//! The thread builds the module and compiles it (`template::build_module`),
//! then calls `make`, which is the part that needs the graphics card: the
//! renderer gives it a function that makes pipelines. The thread never submits
//! to the queue; only the frame's own thread does.

use std::collections::VecDeque;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use lore_shader::{Compiled, ParamBlock, ShaderError};

use super::book::Job;
use super::template::build_module;

/// What the thread made of one job.
pub(crate) struct Done<T> {
    /// The entry the job was for.
    pub(crate) id: u64,
    /// The revision of the text that was built.
    pub(crate) revision: u64,
    /// The made thing and the block that carries its values, or what is wrong.
    pub(crate) outcome: Result<(T, ParamBlock), ShaderError>,
    /// How long the module took to build and compile.
    pub(crate) compiling: Duration,
    /// How long `make` took.
    pub(crate) building: Duration,
}

/// What the thread and the frame share.
struct Shared {
    state: Mutex<State>,
    /// Raised when there is a job to take, or the thread should stop.
    wake: Condvar,
    /// Raised when the thread has finished a job.
    idle: Condvar,
}

struct State {
    queue: VecDeque<Job>,
    /// Whether a job is being built right now.
    building: bool,
    stop: bool,
}

fn locked(state: &Mutex<State>) -> MutexGuard<'_, State> {
    // A panic never happens while the lock is held, but if one did, the
    // queue is still a queue.
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A thread named "lore shaders" that builds what it is asked for, newest
/// text first.
pub(crate) struct Worker<T> {
    shared: Arc<Shared>,
    results: Receiver<Done<T>>,
    thread: Option<JoinHandle<()>>,
}

impl<T: Send + 'static> Worker<T> {
    /// Starts the thread. `make` is called for each job after its module
    /// compiled; it makes the thing that is drawn from it.
    pub(crate) fn new(
        mut make: impl FnMut(&Job, &Compiled, &ParamBlock) -> Result<T, ShaderError> + Send + 'static,
    ) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                queue: VecDeque::new(),
                building: false,
                stop: false,
            }),
            wake: Condvar::new(),
            idle: Condvar::new(),
        });
        let (send, results) = mpsc::channel();
        let theirs = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("lore shaders".to_owned())
            .spawn(move || {
                while let Some(job) = next_job(&theirs) {
                    let done = catch_unwind(AssertUnwindSafe(|| run(&mut make, &job)))
                        .unwrap_or_else(|panic| stopped(&job, &*panic));
                    // The result goes first: once `wait_idle` returns, it is there.
                    let _ = send.send(done);
                    locked(&theirs.state).building = false;
                    theirs.idle.notify_all();
                }
            })?;
        Ok(Self {
            shared,
            results,
            thread: Some(thread),
        })
    }

    /// Asks for a job to be built. A job of the same id that has not started
    /// is replaced, so only the newest text compiles.
    pub(crate) fn ask(&self, job: Job) {
        let mut state = locked(&self.shared.state);
        match state.queue.iter_mut().find(|queued| queued.id == job.id) {
            Some(queued) => *queued = job,
            None => state.queue.push_back(job),
        }
        drop(state);
        self.shared.wake.notify_one();
    }

    /// Drops the jobs of `id` that have not started.
    pub(crate) fn forget(&self, id: u64) {
        locked(&self.shared.state)
            .queue
            .retain(|queued| queued.id != id);
    }

    /// The next result, if there is one. Never waits.
    pub(crate) fn done(&self) -> Option<Done<T>> {
        self.results.try_recv().ok()
    }

    /// Waits until the queue is empty and nothing is being built.
    pub(crate) fn wait_idle(&self) {
        let mut state = locked(&self.shared.state);
        while !(state.queue.is_empty() && !state.building) {
            // Looks in now and then so a thread that is gone cannot hold this for ever.
            if self.thread.as_ref().is_none_or(JoinHandle::is_finished) {
                return;
            }
            state = self
                .shared
                .idle
                .wait_timeout(state, Duration::from_millis(50))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

impl<T> Drop for Worker<T> {
    fn drop(&mut self) {
        locked(&self.shared.state).stop = true;
        self.shared.wake.notify_all();
        if let Some(thread) = self.thread.take() {
            // Its own panics were caught; there is nothing more to learn from it.
            let _ = thread.join();
        }
    }
}

/// Waits for the next job and marks it as being built, or gives `None` when
/// the thread should stop.
fn next_job(shared: &Shared) -> Option<Job> {
    let mut state = locked(&shared.state);
    loop {
        if state.stop {
            return None;
        }
        if let Some(job) = state.queue.pop_front() {
            state.building = true;
            return Some(job);
        }
        state = shared
            .wake
            .wait(state)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

/// Builds one job: the module, then what `make` makes of it.
fn run<T>(
    make: &mut impl FnMut(&Job, &Compiled, &ParamBlock) -> Result<T, ShaderError>,
    job: &Job,
) -> Done<T> {
    let started = Instant::now();
    let built = build_module(job.hook, &job.file, &job.text);
    let compiling = started.elapsed();
    let started = Instant::now();
    let outcome =
        built.and_then(|(compiled, block)| make(job, &compiled, &block).map(|made| (made, block)));
    Done {
        id: job.id,
        revision: job.revision,
        outcome,
        compiling,
        building: started.elapsed(),
    }
}

/// What to say when building a job panicked: a fault in LORE.
fn stopped<T>(job: &Job, panic: &(dyn std::any::Any + Send)) -> Done<T> {
    let what = panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("no reason given");
    Done {
        id: job.id,
        revision: job.revision,
        outcome: Err(ShaderError {
            file: "crates/lore-render/src/game_shaders/worker.rs".to_owned(),
            line: None,
            column: None,
            message: format!("Building {} stopped unexpectedly: {what}.", job.file),
            source_line: None,
            hint: Some(
                "This is a fault in LORE, not in your shader. Please report it at \
                 https://github.com/tahuffman1s/LORE/issues with this whole message."
                    .to_owned(),
            ),
            note: None,
            lore_fault: true,
        }),
        compiling: Duration::ZERO,
        building: Duration::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lore_shader::Hook;
    use std::sync::mpsc::Sender;

    fn job(id: u64, revision: u64) -> Job {
        Job {
            id,
            revision,
            hook: Hook::Screen,
            file: "shaders/test.wgsl".to_owned(),
            text: Hook::Screen.starter().to_owned(),
        }
    }

    /// Takes every result there will be, once the worker is idle.
    fn all(worker: &Worker<u64>) -> Vec<Done<u64>> {
        worker.wait_idle();
        std::iter::from_fn(|| worker.done()).collect()
    }

    #[test]
    fn a_job_is_compiled_and_made() {
        let worker = Worker::new(|job: &Job, compiled: &Compiled, _: &ParamBlock| {
            assert!(!compiled.words.is_empty());
            Ok(job.revision * 10)
        })
        .unwrap();
        worker.ask(job(1, 7));
        let done = all(&worker);
        assert_eq!(done.len(), 1);
        assert_eq!((done[0].id, done[0].revision), (1, 7));
        assert_eq!(done[0].outcome.as_ref().map(|(made, _)| *made), Ok(70));
        assert!(done[0].compiling > Duration::ZERO);
        let _took = done[0].building;
    }

    #[test]
    fn a_broken_shader_comes_back_as_its_error() {
        let worker = Worker::new(|_: &Job, _: &Compiled, _: &ParamBlock| Ok(0u64)).unwrap();
        let mut broken = job(1, 1);
        broken.text = "fn screen(in: ScreenIn) -> vec3f { return in.colour; }".to_owned();
        worker.ask(broken);
        let done = all(&worker);
        let error = done[0].outcome.as_ref().err().unwrap();
        assert_eq!(error.file, "shaders/test.wgsl");
        assert!(!error.lore_fault);
    }

    #[test]
    fn only_the_first_and_the_newest_are_built() {
        let (started_send, started) = mpsc::channel::<u64>();
        let (release, gate) = mpsc::channel::<()>();
        let worker = Worker::new(move |job: &Job, _: &Compiled, _: &ParamBlock| {
            let _ = started_send.send(job.revision);
            if job.revision == 1 {
                let _ = gate.recv();
            }
            Ok(job.revision)
        })
        .unwrap();
        worker.ask(job(1, 1));
        // The first is being built before the others are asked for.
        assert_eq!(started.recv().unwrap(), 1);
        worker.ask(job(1, 2));
        worker.ask(job(1, 3));
        worker.ask(job(1, 4));
        release.send(()).unwrap();
        let revisions: Vec<u64> = all(&worker).iter().map(|done| done.revision).collect();
        assert_eq!(revisions, [1, 4]);
    }

    #[test]
    fn forget_drops_what_has_not_started() {
        let (started_send, started) = mpsc::channel::<u64>();
        let (release, gate) = mpsc::channel::<()>();
        let worker = Worker::new(move |job: &Job, _: &Compiled, _: &ParamBlock| {
            let _ = started_send.send(job.id);
            if job.id == 1 {
                let _ = gate.recv();
            }
            Ok(job.id)
        })
        .unwrap();
        worker.ask(job(1, 1));
        assert_eq!(started.recv().unwrap(), 1);
        worker.ask(job(2, 1));
        worker.ask(job(3, 1));
        worker.forget(2);
        release.send(()).unwrap();
        let ids: Vec<u64> = all(&worker).iter().map(|done| done.id).collect();
        assert_eq!(ids, [1, 3]);
    }

    #[test]
    fn wait_idle_returns_at_once_when_there_is_nothing_to_do() {
        let worker = Worker::new(|_: &Job, _: &Compiled, _: &ParamBlock| Ok(0u64)).unwrap();
        worker.wait_idle();
        assert!(worker.done().is_none());
    }

    #[test]
    fn a_panic_in_make_is_a_fault_and_wait_idle_returns() {
        let worker = Worker::new(|job: &Job, _: &Compiled, _: &ParamBlock| {
            if job.revision == 1 {
                panic!("the card said no");
            }
            Ok(job.revision)
        })
        .unwrap();
        worker.ask(job(1, 1));
        let done = all(&worker);
        let error = done[0].outcome.as_ref().err().unwrap();
        assert!(error.lore_fault);
        assert!(error.message.contains("the card said no"), "{error}");
        // The thread is still there for the next job.
        worker.ask(job(1, 2));
        assert_eq!(all(&worker)[0].revision, 2);
    }

    #[test]
    fn dropping_joins_the_thread() {
        struct Flag(Sender<()>);
        impl Drop for Flag {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let (send, gone) = mpsc::channel();
        let flag = Flag(send);
        let worker = Worker::new(move |_: &Job, _: &Compiled, _: &ParamBlock| {
            let _keep = &flag;
            Ok(0u64)
        })
        .unwrap();
        worker.ask(job(1, 1));
        worker.wait_idle();
        drop(worker);
        // `make` (and what it owned) was dropped by the thread before the drop returned.
        assert!(gone.try_recv().is_ok());
    }
}
