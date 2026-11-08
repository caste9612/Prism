//! The single job engine (ADR-0009): every long operation (indexing, updates, and later
//! hashing, verification, copies) runs here, with a priority, a cancellation token, progress,
//! and at most one heavy job per physical disk.
//!
//! The engine is in memory: the persistent job record and the resume of background jobs after
//! a crash arrive with the background jobs themselves (T6, REVIEW D10); an interrupted indexing
//! is already detected by the volume status (`reindex_needed`, NF4).

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender};

use crate::cancel::CancelToken;
use crate::model::{VolumeId, unix_millis_now};

/// Identifier of a job (unique for the lifetime of the engine).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JobId(pub u64);

/// Scheduling priority: lower value runs first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    /// Requested by the user and waited for.
    Interactive = 0,
    /// Indexing and updates of volumes.
    Indexing = 1,
    /// Hashes, fingerprints: minimum I/O and CPU priority.
    Background = 2,
}

/// State of a job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JobState {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl JobState {
    #[must_use]
    pub fn is_finished(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Progress of a running job, updated by the job and read by the interface.
#[derive(Debug, Default)]
pub struct JobProgress {
    pub done: AtomicU64,
    /// 0 when the total is not known (indeterminate progress).
    pub total: AtomicU64,
    phase: Mutex<String>,
}

impl JobProgress {
    /// Sets a short machine-readable phase (`"scanning"`, `"writing"`...), translated by the UI.
    pub fn set_phase(&self, phase: &str) {
        *self.phase.lock().unwrap_or_else(PoisonError::into_inner) = phase.to_owned();
    }

    #[must_use]
    pub fn phase(&self) -> String {
        self.phase
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// What a running job receives.
#[derive(Clone, Debug)]
pub struct JobContext {
    pub id: JobId,
    pub cancel: CancelToken,
    pub progress: Arc<JobProgress>,
}

type JobFn = Box<dyn FnOnce(&JobContext) -> Result<(), String> + Send>;

/// A job to submit.
pub struct JobSpec {
    /// Machine-readable kind (`"index"`, `"update"`...), translated by the UI.
    pub kind: String,
    pub volume: Option<VolumeId>,
    pub priority: Priority,
    /// Physical disks the job reads heavily; two jobs sharing a disk never run together.
    pub disks: Vec<String>,
    pub run: JobFn,
}

/// A snapshot of a job, for the interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobInfo {
    pub id: JobId,
    pub kind: String,
    pub volume: Option<VolumeId>,
    pub priority: Priority,
    pub state: JobState,
    pub done: u64,
    pub total: u64,
    pub phase: String,
    pub error: Option<String>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

struct Entry {
    info: JobInfo,
    disks: Vec<String>,
    run: Option<JobFn>,
    cancel: CancelToken,
    progress: Arc<JobProgress>,
}

#[derive(Default)]
struct State {
    next_id: u64,
    jobs: VecDeque<Entry>,
    busy_disks: HashSet<String>,
    stopping: bool,
}

/// Finished jobs kept for the interface.
const KEEP_FINISHED: usize = 50;

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    events: Mutex<Vec<Sender<JobInfo>>>,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn publish(&self, info: &JobInfo) {
        let mut subs = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        subs.retain(|s| s.send(info.clone()).is_ok());
    }
}

fn snapshot(e: &Entry) -> JobInfo {
    let mut info = e.info.clone();
    info.done = e.progress.done.load(Ordering::Relaxed);
    info.total = e.progress.total.load(Ordering::Relaxed);
    info.phase = e.progress.phase();
    info
}

/// The job engine: a few worker threads taking the most urgent runnable job.
pub struct JobEngine {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}

impl JobEngine {
    /// Starts `workers` worker threads (at least 1).
    #[must_use]
    pub fn start(workers: usize) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            events: Mutex::new(Vec::new()),
        });
        let workers = (0..workers.max(1))
            .filter_map(|i| {
                let shared = Arc::clone(&shared);
                std::thread::Builder::new()
                    .name(format!("prism-job-{i}"))
                    .spawn(move || worker(&shared))
                    .ok()
            })
            .collect();
        Self { shared, workers }
    }

    /// Queues a job.
    pub fn submit(&self, spec: JobSpec) -> JobId {
        let mut state = self.shared.lock();
        state.next_id += 1;
        let id = JobId(state.next_id);
        let entry = Entry {
            info: JobInfo {
                id,
                kind: spec.kind,
                volume: spec.volume,
                priority: spec.priority,
                state: JobState::Queued,
                done: 0,
                total: 0,
                phase: String::new(),
                error: None,
                created_at: unix_millis_now(),
                started_at: None,
                finished_at: None,
            },
            disks: spec.disks,
            run: Some(spec.run),
            cancel: CancelToken::new(),
            progress: Arc::new(JobProgress::default()),
        };
        let info = snapshot(&entry);
        state.jobs.push_back(entry);
        drop(state);
        self.shared.publish(&info);
        self.shared.wake.notify_all();
        id
    }

    /// Cancels a job: a queued job is dropped, a running job is asked to stop.
    pub fn cancel(&self, id: JobId) -> bool {
        let mut state = self.shared.lock();
        let Some(e) = state.jobs.iter_mut().find(|e| e.info.id == id) else {
            return false;
        };
        match e.info.state {
            JobState::Queued => {
                e.info.state = JobState::Cancelled;
                e.info.finished_at = Some(unix_millis_now());
                e.run = None;
                let info = snapshot(e);
                drop(state);
                self.shared.publish(&info);
                true
            }
            JobState::Running => {
                e.cancel.cancel();
                true
            }
            _ => false,
        }
    }

    /// Every job known to the engine (queued, running, recently finished).
    #[must_use]
    pub fn jobs(&self) -> Vec<JobInfo> {
        self.shared.lock().jobs.iter().map(snapshot).collect()
    }

    /// The running or queued jobs of a volume.
    #[must_use]
    pub fn active_for(&self, volume: VolumeId) -> Vec<JobInfo> {
        self.jobs()
            .into_iter()
            .filter(|j| j.volume == Some(volume) && !j.state.is_finished())
            .collect()
    }

    /// Whether running job `id` should stop to let a more urgent queued job use one of its
    /// disks: a background job (hashes) gives way to indexing instead of making it wait hours.
    #[must_use]
    pub fn should_yield(&self, id: JobId) -> bool {
        let state = self.shared.lock();
        let Some(me) = state.jobs.iter().find(|e| e.info.id == id) else {
            return false;
        };
        state.jobs.iter().any(|e| {
            e.info.state == JobState::Queued
                && e.info.priority < me.info.priority
                && e.disks.iter().any(|d| me.disks.contains(d))
        })
    }

    /// Receives a snapshot whenever a job is queued, starts or finishes. Progress is read with
    /// [`JobEngine::jobs`] (the interface polls it at a limited rate, ADR-0009).
    #[must_use]
    pub fn subscribe(&self) -> Receiver<JobInfo> {
        let (tx, rx) = crossbeam_channel::unbounded();
        self.shared
            .events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(tx);
        rx
    }

    /// Waits until job `id` is finished and returns its final state (for tests and tools).
    #[must_use]
    pub fn wait(&self, id: JobId) -> Option<JobInfo> {
        let mut state = self.shared.lock();
        loop {
            let info = state.jobs.iter().find(|e| e.info.id == id).map(snapshot)?;
            if info.state.is_finished() {
                return Some(info);
            }
            state = self
                .shared
                .wake
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl Drop for JobEngine {
    fn drop(&mut self) {
        {
            let mut state = self.shared.lock();
            state.stopping = true;
            for e in &state.jobs {
                e.cancel.cancel();
            }
        }
        self.shared.wake.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

fn worker(shared: &Shared) {
    loop {
        let (run, ctx, disks) = {
            let mut state = shared.lock();
            loop {
                if state.stopping {
                    return;
                }
                // The most urgent queued job whose disks are free (FIFO within a priority).
                let busy = &state.busy_disks;
                let pick = state
                    .jobs
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.info.state == JobState::Queued)
                    .filter(|(_, e)| e.disks.iter().all(|d| !busy.contains(d)))
                    .min_by_key(|(i, e)| (e.info.priority, *i))
                    .map(|(i, _)| i);
                if let Some(i) = pick {
                    let disks = state.jobs[i].disks.clone();
                    for d in &disks {
                        state.busy_disks.insert(d.clone());
                    }
                    let e = &mut state.jobs[i];
                    e.info.state = JobState::Running;
                    e.info.started_at = Some(unix_millis_now());
                    let ctx = JobContext {
                        id: e.info.id,
                        cancel: e.cancel.clone(),
                        progress: Arc::clone(&e.progress),
                    };
                    let run = e.run.take();
                    let info = snapshot(e);
                    drop(state);
                    shared.publish(&info);
                    break (run, ctx, disks);
                }
                state = shared
                    .wake
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        let outcome = match run {
            Some(run) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&ctx)))
                .unwrap_or_else(|_| Err("the job panicked".to_owned())),
            None => Err("the job had nothing to run".to_owned()),
        };
        let mut state = shared.lock();
        for d in &disks {
            state.busy_disks.remove(d);
        }
        let info = state
            .jobs
            .iter_mut()
            .find(|e| e.info.id == ctx.id)
            .map(|e| {
                e.info.finished_at = Some(unix_millis_now());
                match &outcome {
                    Ok(()) => e.info.state = JobState::Done,
                    Err(_) if ctx.cancel.is_cancelled() => e.info.state = JobState::Cancelled,
                    Err(msg) => {
                        e.info.state = JobState::Failed;
                        e.info.error = Some(msg.clone());
                    }
                }
                snapshot(e)
            });
        // Forget the oldest finished jobs.
        let finished = state
            .jobs
            .iter()
            .filter(|e| e.info.state.is_finished())
            .count();
        if finished > KEEP_FINISHED
            && let Some(pos) = state.jobs.iter().position(|e| e.info.state.is_finished())
        {
            state.jobs.remove(pos);
        }
        drop(state);
        if let Some(info) = info {
            shared.publish(&info);
        }
        shared.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn spec(kind: &str, priority: Priority, disks: &[&str], run: JobFn) -> JobSpec {
        JobSpec {
            kind: kind.to_owned(),
            volume: None,
            priority,
            disks: disks.iter().map(|d| (*d).to_owned()).collect(),
            run,
        }
    }

    #[test]
    fn runs_jobs_and_reports_their_outcome() {
        let engine = JobEngine::start(2);
        let ok = engine.submit(spec("ok", Priority::Indexing, &[], Box::new(|_| Ok(()))));
        let ko = engine.submit(spec(
            "ko",
            Priority::Indexing,
            &[],
            Box::new(|_| Err("boom".into())),
        ));
        let panics = engine.submit(spec(
            "panics",
            Priority::Indexing,
            &[],
            Box::new(|_| panic!("job panic")),
        ));
        assert_eq!(engine.wait(ok).unwrap().state, JobState::Done);
        let failed = engine.wait(ko).unwrap();
        assert_eq!(failed.state, JobState::Failed);
        assert_eq!(failed.error.as_deref(), Some("boom"));
        assert_eq!(engine.wait(panics).unwrap().state, JobState::Failed);
    }

    // ADR-0009: interactive before indexing before background; FIFO within a priority.
    #[test]
    fn higher_priority_runs_first() {
        let engine = JobEngine::start(1);
        let order = Arc::new(Mutex::new(Vec::new()));
        let (gate_tx, gate_rx) = crossbeam_channel::bounded::<()>(0);
        // Occupy the only worker until every job is queued.
        let blocker = engine.submit(spec(
            "blocker",
            Priority::Interactive,
            &[],
            Box::new(move |_| {
                let _ = gate_rx.recv();
                Ok(())
            }),
        ));
        let mut ids = Vec::new();
        for (name, p) in [
            ("bg", Priority::Background),
            ("idx1", Priority::Indexing),
            ("ui", Priority::Interactive),
            ("idx2", Priority::Indexing),
        ] {
            let o = Arc::clone(&order);
            ids.push(engine.submit(spec(
                name,
                p,
                &[],
                Box::new(move |_| {
                    o.lock().unwrap().push(name);
                    Ok(())
                }),
            )));
        }
        gate_tx.send(()).unwrap();
        let _ = engine.wait(blocker);
        for id in ids {
            let _ = engine.wait(id);
        }
        assert_eq!(*order.lock().unwrap(), vec!["ui", "idx1", "idx2", "bg"]);
    }

    // ADR-0009: two jobs on the same physical disk never run together.
    #[test]
    fn one_heavy_job_per_disk() {
        let engine = JobEngine::start(4);
        let running = Arc::new(AtomicU64::new(0));
        let max_seen = Arc::new(AtomicU64::new(0));
        let mut ids = Vec::new();
        for _ in 0..4 {
            let r = Arc::clone(&running);
            let m = Arc::clone(&max_seen);
            ids.push(engine.submit(spec(
                "disk0",
                Priority::Indexing,
                &["disk0"],
                Box::new(move |_| {
                    let now = r.fetch_add(1, Ordering::SeqCst) + 1;
                    m.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(20));
                    r.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                }),
            )));
        }
        // A job on another disk runs meanwhile.
        let other = engine.submit(spec(
            "disk1",
            Priority::Indexing,
            &["disk1"],
            Box::new(|_| Ok(())),
        ));
        assert_eq!(engine.wait(other).unwrap().state, JobState::Done);
        for id in ids {
            assert_eq!(engine.wait(id).unwrap().state, JobState::Done);
        }
        assert_eq!(max_seen.load(Ordering::SeqCst), 1);
    }

    // A background job sees that a more urgent job waits for its disk, and only then.
    #[test]
    fn background_jobs_yield_their_disk() {
        let engine = Arc::new(JobEngine::start(4));
        let (started_tx, started_rx) = crossbeam_channel::bounded::<()>(1);
        let (seen_tx, seen_rx) = crossbeam_channel::bounded::<()>(1);
        let e = Arc::clone(&engine);
        let bg = engine.submit(spec(
            "hash",
            Priority::Background,
            &["disk0"],
            Box::new(move |ctx| {
                started_tx.send(()).unwrap();
                while !e.should_yield(ctx.id) {
                    std::thread::sleep(Duration::from_millis(1));
                }
                seen_tx.send(()).unwrap();
                Ok(())
            }),
        ));
        started_rx.recv().unwrap();
        // Another disk, or the same priority, do not matter.
        let other = engine.submit(spec(
            "other",
            Priority::Indexing,
            &["disk1"],
            Box::new(|_| Ok(())),
        ));
        assert_eq!(engine.wait(other).unwrap().state, JobState::Done);
        assert!(!engine.should_yield(bg));
        let same = engine.submit(spec(
            "hash2",
            Priority::Background,
            &["disk0"],
            Box::new(|_| Ok(())),
        ));
        std::thread::sleep(Duration::from_millis(20));
        assert!(!engine.should_yield(bg));
        assert!(seen_rx.try_recv().is_err());
        // An update of the same disk: the background job stops, then the update runs.
        let update = engine.submit(spec(
            "update",
            Priority::Indexing,
            &["disk0"],
            Box::new(|_| Ok(())),
        ));
        seen_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(engine.wait(bg).unwrap().state, JobState::Done);
        assert_eq!(engine.wait(update).unwrap().state, JobState::Done);
        assert_eq!(engine.wait(same).unwrap().state, JobState::Done);
        assert!(!engine.should_yield(JobId(9999)));
    }

    #[test]
    fn cancelling_queued_and_running_jobs() {
        let engine = JobEngine::start(1);
        let (started_tx, started_rx) = crossbeam_channel::bounded::<()>(1);
        let running = engine.submit(spec(
            "long",
            Priority::Indexing,
            &[],
            Box::new(move |ctx| {
                ctx.progress.set_phase("scanning");
                ctx.progress.total.store(100, Ordering::Relaxed);
                started_tx.send(()).unwrap();
                while !ctx.cancel.is_cancelled() {
                    ctx.progress.done.fetch_add(1, Ordering::Relaxed);
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err("cancelled".into())
            }),
        ));
        let queued = engine.submit(spec(
            "queued",
            Priority::Indexing,
            &[],
            Box::new(|_| Ok(())),
        ));
        started_rx.recv().unwrap();
        let snapshot = engine.jobs();
        let live = snapshot.iter().find(|j| j.id == running).unwrap();
        assert_eq!(live.state, JobState::Running);
        assert_eq!(live.phase, "scanning");
        assert_eq!(live.total, 100);
        assert!(engine.cancel(queued));
        assert_eq!(engine.wait(queued).unwrap().state, JobState::Cancelled);
        assert!(engine.cancel(running));
        assert_eq!(engine.wait(running).unwrap().state, JobState::Cancelled);
        assert!(!engine.cancel(running), "already finished");
    }

    #[test]
    fn events_follow_the_job_lifecycle() {
        let engine = JobEngine::start(1);
        let events = engine.subscribe();
        let id = engine.submit(spec("x", Priority::Indexing, &[], Box::new(|_| Ok(()))));
        let _ = engine.wait(id);
        let states: Vec<JobState> = events.try_iter().map(|j| j.state).collect();
        assert_eq!(
            states,
            vec![JobState::Queued, JobState::Running, JobState::Done]
        );
    }
}
