//! One background reader with cooperative cancellation and a bounded memory cache.
use crate::{history::History, session::Session};
use anyhow::{Context, Result, bail};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

pub type Progress<'a> = dyn FnMut(&str, usize) -> Result<()> + 'a;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    History,
    OpenSession,
    ReloadSession,
    PrepareResume,
}

pub enum Job {
    History(Vec<PathBuf>),
    OpenSession {
        directory: PathBuf,
        id: String,
        origin: Option<PathBuf>,
    },
    ReloadSession(PathBuf),
    PrepareResume {
        plan: crate::resume::Plan,
        settings: crate::resume::Settings,
        directory: PathBuf,
        session_path: Option<PathBuf>,
    },
}
impl Job {
    pub fn target(&self) -> Target {
        match self {
            Self::History(_) => Target::History,
            Self::OpenSession { .. } => Target::OpenSession,
            Self::ReloadSession(_) => Target::ReloadSession,
            Self::PrepareResume { .. } => Target::PrepareResume,
        }
    }
}

pub enum Loaded {
    Resume(crate::resume::Ready),
    History(History),
    Session {
        session: Session,
        cached: bool,
        origin: Option<PathBuf>,
    },
}
pub enum Event {
    Progress {
        target: Target,
        message: String,
    },
    Finished {
        target: Target,
        result: std::result::Result<Loaded, String>,
    },
}
struct Request {
    id: u64,
    job: Job,
}
struct Envelope {
    id: u64,
    event: Event,
}

pub struct Loader {
    requests: Option<mpsc::Sender<Request>>,
    events: mpsc::Receiver<Envelope>,
    current: Arc<AtomicU64>,
    pub pending: Option<Target>,
}
impl Loader {
    pub fn new() -> Result<Self> {
        let (requests, rx) = mpsc::channel::<Request>();
        let (tx, events) = mpsc::channel();
        let current = Arc::new(AtomicU64::new(0));
        let generation = current.clone();
        thread::Builder::new()
            .name("history-loader".into())
            .spawn(move || {
                let mut cache = SessionCache::new(4, 64 * 1024 * 1024);
                while let Ok(mut request) = rx.recv() {
                    // Superseded requests need no I/O at all.
                    while let Ok(newer) = rx.try_recv() {
                        request = newer;
                    }
                    if generation.load(Ordering::Relaxed) != request.id {
                        continue;
                    }
                    let target = request.job.target();
                    let mut last: Option<(String, Instant)> = None;
                    let mut progress = |phase: &str, count: usize| -> Result<()> {
                        if generation.load(Ordering::Relaxed) != request.id {
                            bail!("Loading cancelled");
                        }
                        if last.as_ref().is_none_or(|(previous, time)| {
                            previous != phase || time.elapsed() >= Duration::from_millis(100)
                        }) {
                            tx.send(Envelope {
                                id: request.id,
                                event: Event::Progress {
                                    target,
                                    message: format!("{phase} · {count} · Esc cancels"),
                                },
                            })
                            .context("UI closed")?;
                            last = Some((phase.to_owned(), Instant::now()));
                        }
                        Ok(())
                    };
                    let result = match request.job {
                        Job::PrepareResume {
                            plan,
                            settings,
                            directory,
                            session_path,
                        } => settings
                            .prepare(plan, &directory, session_path.as_deref(), &mut progress)
                            .map(Loaded::Resume),
                        Job::History(paths) => {
                            History::load_many_with(&paths, &mut progress).map(Loaded::History)
                        }
                        Job::OpenSession {
                            directory,
                            id,
                            origin,
                        } => cache
                            .open(&directory, &id, &mut progress)
                            .map(|(session, cached)| Loaded::Session {
                                session,
                                cached,
                                origin,
                            }),
                        Job::ReloadSession(path) => {
                            cache
                                .load(&path, true, &mut progress)
                                .map(|(session, cached)| Loaded::Session {
                                    session,
                                    cached,
                                    origin: None,
                                })
                        }
                    }
                    .map_err(|error| format!("{error:#}"));
                    if generation.load(Ordering::Relaxed) == request.id
                        && tx
                            .send(Envelope {
                                id: request.id,
                                event: Event::Finished { target, result },
                            })
                            .is_err()
                    {
                        break;
                    }
                }
            })
            .context("Cannot start background loader")?;
        Ok(Self {
            requests: Some(requests),
            events,
            current,
            pending: None,
        })
    }

    pub fn start(&mut self, job: Job) -> Result<()> {
        let id = self.current.fetch_add(1, Ordering::Relaxed) + 1;
        let target = job.target();
        self.requests
            .as_ref()
            .context("Loader closed")?
            .send(Request { id, job })
            .context("Background loader stopped")?;
        self.pending = Some(target);
        Ok(())
    }

    pub fn cancel(&mut self) {
        self.current.fetch_add(1, Ordering::Relaxed);
        self.pending = None;
    }

    pub fn poll(&mut self) -> Option<Event> {
        loop {
            match self.events.try_recv() {
                Ok(envelope) if envelope.id == self.current.load(Ordering::Relaxed) => {
                    if matches!(&envelope.event, Event::Finished { .. }) {
                        self.pending = None;
                    }
                    return Some(envelope.event);
                }
                Ok(_) => continue,
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    return self.pending.take().map(|target| Event::Finished {
                        target,
                        result: Err("Background loader stopped".into()),
                    });
                }
            }
        }
    }
}
impl Drop for Loader {
    fn drop(&mut self) {
        self.cancel();
        self.requests.take();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: Option<SystemTime>,
}
impl Stamp {
    fn read(path: &Path) -> Result<Self> {
        let metadata =
            fs::metadata(path).with_context(|| format!("Cannot inspect {}", path.display()))?;
        Ok(Self {
            size: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
}
struct Cached {
    path: PathBuf,
    stamp: Stamp,
    session: Session,
    bytes: usize,
}
struct SessionCache {
    entries: VecDeque<Cached>,
    paths: HashMap<(PathBuf, String), PathBuf>,
    bytes: usize,
    limit: usize,
    budget: usize,
}
impl SessionCache {
    fn new(limit: usize, budget: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            paths: HashMap::new(),
            bytes: 0,
            limit,
            budget,
        }
    }

    fn open(
        &mut self,
        directory: &Path,
        id: &str,
        progress: &mut Progress<'_>,
    ) -> Result<(Session, bool)> {
        progress("Locating session", 0)?;
        let directory = directory
            .canonicalize()
            .with_context(|| format!("Cannot read {}", directory.display()))?;
        let key = (directory.clone(), id.to_owned());
        let path = match self
            .paths
            .get(&key)
            .filter(|path| crate::session::matches_id(path, id).unwrap_or(false))
        {
            Some(path) => path.clone(),
            None => crate::session::find_with(&directory, id, progress)?,
        };
        if self.paths.len() >= 128 {
            self.paths.clear();
        }
        self.paths.insert(key, path.clone());
        self.load(&path, false, progress)
    }

    fn load(
        &mut self,
        path: &Path,
        force: bool,
        progress: &mut Progress<'_>,
    ) -> Result<(Session, bool)> {
        progress("Checking session file", 0)?;
        let path = path
            .canonicalize()
            .with_context(|| format!("Cannot open {}", path.display()))?;
        let before = Stamp::read(&path)?;
        if let Some(index) = self.entries.iter().position(|entry| entry.path == path) {
            let cached = self.entries.remove(index).unwrap();
            self.bytes -= cached.bytes;
            if !force && before.modified.is_some() && cached.stamp == before {
                progress(
                    "Opening cached session",
                    cached.session.history.entries.len(),
                )?;
                let session = cached.session.clone();
                self.bytes += cached.bytes;
                self.entries.push_back(cached);
                return Ok((session, true));
            }
        }
        let session = Session::load(&path, progress)?;
        let after = Stamp::read(&path)?;
        progress("Session ready", session.history.entries.len())?;
        let bytes = session_bytes(&session);
        // A changing file can be displayed, but must not populate a stale cache.
        if before == after && after.modified.is_some() && bytes <= self.budget && self.limit > 0 {
            while self.entries.len() >= self.limit || self.bytes + bytes > self.budget {
                if let Some(old) = self.entries.pop_front() {
                    self.bytes -= old.bytes;
                } else {
                    break;
                }
            }
            self.entries.push_back(Cached {
                path,
                stamp: after,
                session: session.clone(),
                bytes,
            });
            self.bytes += bytes;
        }
        Ok((session, false))
    }
}
fn session_bytes(session: &Session) -> usize {
    session.id.capacity()
        + session.info.capacity()
        + session.history.entries.capacity() * std::mem::size_of::<crate::history::Entry>()
        + session
            .history
            .entries
            .iter()
            .map(|entry| {
                entry.text.capacity()
                    + entry.session_id.capacity()
                    + entry
                        .tool
                        .as_ref()
                        .map_or(0, |tool| tool.summary.capacity())
            })
            .sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "codex-loader-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn session(&self, name: &str, id: &str, text: &str) -> PathBuf {
            let dir = self.0.join(name);
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("session.jsonl");
            fs::write(&path, format!("{}\n{}\n", serde_json::json!({"type":"session_meta","payload":{"id":id}}), serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}}))).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn cache_reuses_unchanged_sessions_and_invalidates_on_reload_or_change() {
        let fixture = Fixture::new();
        let path = fixture.session("alpha", "same", "original");
        let mut cache = SessionCache::new(4, 1024 * 1024);
        let mut progress = |_: &str, _: usize| Ok(());
        let (mut first, hit) = cache
            .open(path.parent().unwrap(), "same", &mut progress)
            .unwrap();
        assert!(!hit);
        first.history.entries[0].text = "modified view".into();
        let (second, hit) = cache
            .open(path.parent().unwrap(), "same", &mut progress)
            .unwrap();
        assert!(hit);
        assert_eq!(second.history.entries[0].text, "original");
        assert!(!cache.load(&path, true, &mut progress).unwrap().1);
        fixture.session("alpha", "same", "changed with a different size");
        let (changed, hit) = cache
            .open(path.parent().unwrap(), "same", &mut progress)
            .unwrap();
        assert!(!hit);
        assert!(changed.history.entries[0].text.starts_with("changed"));
    }

    #[test]
    fn cache_bounds_and_source_identity_are_respected() {
        let fixture = Fixture::new();
        let a = fixture.session("alpha", "same", "alpha");
        let b = fixture.session("beta", "same", "beta");
        let mut cache = SessionCache::new(1, 1024 * 1024);
        let mut progress = |_: &str, _: usize| Ok(());
        cache
            .open(a.parent().unwrap(), "same", &mut progress)
            .unwrap();
        assert_eq!(
            cache
                .open(b.parent().unwrap(), "same", &mut progress)
                .unwrap()
                .0
                .history
                .entries[0]
                .text,
            "beta"
        );
        assert_eq!(cache.entries.len(), 1);
        assert!(
            !cache
                .open(a.parent().unwrap(), "same", &mut progress)
                .unwrap()
                .1
        );
        let mut tiny = SessionCache::new(4, 1);
        tiny.load(&a, false, &mut progress).unwrap();
        assert!(tiny.entries.is_empty());
        assert_eq!(tiny.bytes, 0);
    }

    #[test]
    fn parsers_cooperatively_stop_when_progress_is_cancelled() {
        let fixture = Fixture::new();
        let path = fixture.session("alpha", "same", "text");
        let error = Session::load(&path, &mut |_, count| {
            if count >= 2 {
                bail!("cancel test");
            }
            Ok(())
        })
        .err()
        .unwrap();
        assert!(error.to_string().contains("cancel test"));
        let history = fixture.0.join("history.jsonl");
        fs::write(
            &history,
            "{\"session_id\":\"a\",\"ts\":1,\"text\":\"a\"}\n\n",
        )
        .unwrap();
        assert!(
            History::load_many_with(&[history], &mut |_, count| {
                if count >= 2 {
                    bail!("cancel history");
                }
                Ok(())
            })
            .is_err()
        );
    }

    #[test]
    fn only_the_latest_request_can_complete_and_cancel_discards_results() {
        let fixture = Fixture::new();
        let path = fixture.0.join("history.jsonl");
        fs::write(
            &path,
            "{\"session_id\":\"a\",\"ts\":1,\"text\":\"latest\"}\n",
        )
        .unwrap();
        let mut loader = Loader::new().unwrap();
        loader
            .start(Job::History(vec![fixture.0.join("missing")]))
            .unwrap();
        loader.start(Job::History(vec![path.clone()])).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(Event::Finished { result, .. }) = loader.poll() {
                let Loaded::History(history) = result.unwrap() else {
                    panic!("wrong payload");
                };
                assert_eq!(history.entries[0].text, "latest");
                break;
            }
            assert!(Instant::now() < until, "background load timed out");
            thread::sleep(Duration::from_millis(2));
        }
        loader.start(Job::History(vec![path])).unwrap();
        loader.cancel();
        thread::sleep(Duration::from_millis(20));
        assert!(loader.poll().is_none());
        assert!(loader.pending.is_none());
    }
}
