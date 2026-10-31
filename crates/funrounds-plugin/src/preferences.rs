// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Knock {
    Off,
    #[default]
    Airborne,
    Always,
}

impl Knock {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Airborne,
            Self::Airborne => Self::Always,
            Self::Always => Self::Off,
        }
    }

    pub fn permits(self, flags: i32) -> bool {
        self == Self::Always || (self == Self::Airborne && flags & 1 == 0)
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "knock.off",
            Self::Airborne => "knock.airborne",
            Self::Always => "knock.always",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreferenceFeedback {
    pub value: Knock,
    pub persistence: PreferencePersistence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreferencePersistence {
    SessionOnly,
    Unchanged,
    Queued,
    Failed(String),
}

pub fn authenticated_id(slot: i32) -> Option<u64> {
    use crate::s2sdk;
    if slot < 0 || slot >= s2sdk::GetMaxClients() || !s2sdk::IsClientInGame(slot) || s2sdk::IsFakeClient(slot) || !s2sdk::IsClientAuthorized(slot) {
        return None;
    }
    let id = s2sdk::GetClientSteamID64(slot);
    (id != 0).then_some(id)
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    knock: BTreeMap<String, Knock>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Change {
    value: Knock,
    revision: u64,
}

struct Session {
    id: Option<u64>,
    value: Knock,
    changed: bool,
}

enum Request {
    Save(BTreeMap<u64, Change>),
    Drain(BTreeMap<u64, Change>, SyncSender<Result<(), String>>),
}

struct BatchResult {
    snapshot: Option<Saved>,
    changes: Vec<(u64, Change, Result<(), String>)>,
}

enum WorkerEvent {
    Loaded(Result<Saved, String>),
    Batch(BatchResult),
}

struct Worker {
    requests: Sender<Request>,
    results: Receiver<WorkerEvent>,
    join: Option<JoinHandle<()>>,
}

impl Worker {
    fn start(path: PathBuf) -> Result<Self, String> {
        let (requests, request_rx) = mpsc::channel();
        let (event_tx, results) = mpsc::channel();
        let join = thread::Builder::new()
            .name("funrounds-player-preferences".into())
            .spawn(move || run_worker(path, request_rx, event_tx))
            .map_err(|_| "failed to start preferences worker".to_owned())?;
        Ok(Self {
            requests,
            results,
            join: Some(join),
        })
    }

    fn save(&self, changes: BTreeMap<u64, Change>) -> Result<(), String> {
        self.requests
            .send(Request::Save(changes))
            .map_err(|_| "preferences worker unavailable".into())
    }

    fn finish(mut self, changes: BTreeMap<u64, Change>) -> (Result<(), String>, Vec<WorkerEvent>) {
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        let sent = self.requests.send(Request::Drain(changes, done_tx)).is_ok();
        let mut result = if sent {
            done_rx.recv().unwrap_or_else(|_| Err("preferences worker exited unexpectedly".into()))
        } else {
            Err("preferences worker unavailable".into())
        };
        if self.join.take().is_some_and(|join| join.join().is_err()) {
            result = Err("preferences worker exited unexpectedly".into());
        }
        let results = self.results.try_iter().collect();
        (result, results)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            let (done_tx, _done_rx) = mpsc::sync_channel(1);
            let _ = self.requests.send(Request::Drain(BTreeMap::new(), done_tx));
            let _ = join.join();
        }
    }
}

pub struct Preferences {
    path: Option<PathBuf>,
    saved: Result<Saved, String>,
    loaded: bool,
    load_error_reported: bool,
    worker: Option<Worker>,
    sessions: HashMap<i32, Session>,
    pending: BTreeMap<u64, Change>,
    failures: HashMap<u64, (u64, String)>,
    reported_failures: HashMap<u64, u64>,
    revision: u64,
}

impl Preferences {
    pub fn new() -> Self {
        let directory = plugify::get_configs_dir().to_string();
        Self::at((!directory.is_empty()).then(|| Path::new(&directory).join("fr_players.toml")))
    }

    fn at(path: Option<PathBuf>) -> Self {
        let (worker, saved, loaded) = match path.as_ref() {
            Some(path) => match Worker::start(path.clone()) {
                Ok(worker) => (Some(worker), Ok(Saved::default()), false),
                Err(error) => (None, Err(error), true),
            },
            None => (None, Err("plugin config directory unavailable".into()), true),
        };
        Self {
            path,
            saved,
            loaded,
            load_error_reported: false,
            worker,
            sessions: HashMap::new(),
            pending: BTreeMap::new(),
            failures: HashMap::new(),
            reported_failures: HashMap::new(),
            revision: 0,
        }
    }

    fn value_from_saved(&self, id: u64) -> Option<Knock> {
        self.pending.get(&id).map(|change| change.value).or_else(|| {
            if self.loaded {
                self.saved
                    .as_ref()
                    .ok()
                    .map(|saved| saved.knock.get(&id.to_string()).copied().unwrap_or_default())
            } else {
                None
            }
        })
    }

    fn apply_snapshot(&mut self, saved: Saved) {
        self.saved = Ok(saved.clone());
        self.loaded = true;
        for session in self.sessions.values_mut() {
            if let Some(id) = session.id
                && !session.changed
                && !self.pending.contains_key(&id)
            {
                session.value = saved.knock.get(&id.to_string()).copied().unwrap_or_default();
            }
        }
    }

    fn apply_result(&mut self, result: BatchResult) {
        if let Some(saved) = result.snapshot {
            self.apply_snapshot(saved);
        }
        for (id, change, status) in result.changes {
            if self.pending.get(&id) != Some(&change) {
                continue;
            }
            match status {
                Ok(()) => {
                    self.pending.remove(&id);
                    self.failures.remove(&id);
                    self.reported_failures.remove(&id);
                }
                Err(error) => {
                    self.failures.insert(id, (change.revision, error.clone()));
                }
            }
        }
    }

    fn apply_event(&mut self, event: WorkerEvent) {
        match event {
            WorkerEvent::Loaded(Ok(saved)) => self.apply_snapshot(saved),
            WorkerEvent::Loaded(Err(error)) => {
                self.saved = Err(error);
                self.loaded = true;
            }
            WorkerEvent::Batch(result) => self.apply_result(result),
        }
    }

    fn poll(&mut self) {
        while let Some(event) = self.worker.as_ref().and_then(|worker| worker.results.try_recv().ok()) {
            self.apply_event(event);
        }
    }

    pub fn check_worker(&mut self) -> Vec<(Option<u64>, String)> {
        self.poll();
        let mut failures = Vec::new();
        if self.loaded && !self.load_error_reported {
            if let Err(error) = &self.saved {
                failures.push((None, error.clone()));
            }
            self.load_error_reported = true;
        }
        let account_failures: Vec<_> = self
            .failures
            .iter()
            .filter(|(id, (revision, _))| self.reported_failures.get(id) != Some(revision))
            .map(|(id, (revision, error))| (*id, *revision, error.clone()))
            .collect();
        for (id, revision, _) in &account_failures {
            self.reported_failures.insert(*id, *revision);
        }
        failures.extend(account_failures.into_iter().map(|(id, _, error)| (Some(id), error)));
        failures
    }

    fn ensure_worker(&mut self) -> Result<(), String> {
        if self.worker.is_some() {
            return Ok(());
        }
        let path = self.path.as_ref().ok_or_else(|| "plugin config directory unavailable".to_owned())?;
        let worker = Worker::start(path.clone())?;
        self.saved = Ok(Saved::default());
        self.loaded = false;
        self.load_error_reported = false;
        self.worker = Some(worker);
        Ok(())
    }

    fn queue_pending(&mut self) -> Result<(), String> {
        if self.pending.is_empty() {
            return Ok(());
        }
        self.ensure_worker()?;
        self.worker.as_ref().expect("worker started").save(self.pending.clone())
    }

    pub fn value(&mut self, slot: i32, id: Option<u64>) -> Knock {
        self.poll();
        let id = id.filter(|id| *id != 0);
        let saved = id.and_then(|id| self.value_from_saved(id)).unwrap_or_default();
        let session = self.sessions.entry(slot).or_insert(Session {
            id,
            value: saved,
            changed: false,
        });
        if session.id != id {
            // Authentication may finish after connect; a session-only choice stays in this connection.
            if session.id.is_some() || !session.changed {
                session.value = saved;
                session.changed = false;
            }
            session.id = id;
        }
        session.value
    }

    pub fn set(&mut self, slot: i32, id: Option<u64>, requested: Option<Knock>) -> PreferenceFeedback {
        let current = self.value(slot, id);
        let value = requested.unwrap_or_else(|| current.next());
        let id = id.filter(|id| *id != 0);
        let changed = value != current;
        let Some(id) = id else {
            let session = self.sessions.get_mut(&slot).expect("session initialized");
            if changed {
                session.value = value;
                session.changed = true;
            }
            return PreferenceFeedback {
                value,
                persistence: PreferencePersistence::SessionOnly,
            };
        };
        if changed {
            for session in self.sessions.values_mut().filter(|session| session.id == Some(id)) {
                session.value = value;
                session.changed = true;
            }
        }

        let already_pending = self.pending.get(&id).is_some_and(|change| change.value == value);
        let needs_save = self.value_from_saved(id).is_none_or(|saved| saved != value);
        if !changed && !needs_save && !already_pending {
            return PreferenceFeedback {
                value,
                persistence: PreferencePersistence::Unchanged,
            };
        }
        if !already_pending {
            self.revision = self.revision.wrapping_add(1);
            self.pending.insert(
                id,
                Change {
                    value,
                    revision: self.revision,
                },
            );
            self.failures.remove(&id);
            self.reported_failures.remove(&id);
            if let Err(error) = self.queue_pending() {
                self.failures.insert(id, (self.revision, error.clone()));
                self.reported_failures.insert(id, self.revision);
            }
        }
        if let Some((revision, error)) = self.failures.get(&id)
            && self.pending.get(&id).is_some_and(|change| change.revision == *revision)
        {
            self.reported_failures.insert(id, *revision);
            return PreferenceFeedback {
                value,
                persistence: PreferencePersistence::Failed(error.clone()),
            };
        }
        PreferenceFeedback {
            value,
            persistence: PreferencePersistence::Queued,
        }
    }

    pub fn forget(&mut self, slot: i32) {
        self.sessions.remove(&slot);
    }

    pub fn clear_sessions(&mut self) {
        self.sessions.clear();
    }

    pub fn shutdown(&mut self) -> Result<(), String> {
        self.poll();
        if self.worker.is_none() && self.pending.is_empty() {
            return Ok(());
        }
        if self.worker.is_none() {
            self.ensure_worker()?;
        }
        let worker = self.worker.take().expect("worker available");
        let (result, results) = worker.finish(self.pending.clone());
        for event in results {
            self.apply_event(event);
        }
        result?;
        if self.pending.is_empty() {
            Ok(())
        } else {
            Err("unsaved player preferences remain queued for retry".into())
        }
    }
}

fn read(path: &Path) -> Result<Saved, String> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Saved::default()),
        Err(_) => return Err("failed to read preferences file".into()),
    };
    // Parser diagnostics can contain account keys; never send them to chat or server logs.
    let saved: Saved = toml::from_str(&source).map_err(|_| "preferences file is corrupt; administrator repair required")?;
    if saved
        .knock
        .keys()
        .any(|key| key.parse::<u64>().ok().is_none_or(|id| id == 0 || id.to_string() != *key))
    {
        return Err("preferences file contains an invalid account key; administrator repair required".into());
    }
    Ok(saved)
}

fn write(path: &Path, saved: &Saved) -> Result<(), String> {
    let source = toml::to_string(saved).map_err(|_| "failed to encode preferences file")?;
    let parent = path.parent().ok_or("preferences directory unavailable")?;
    fs::create_dir_all(parent).map_err(|_| "failed to create preferences directory")?;
    let temporary = path.with_extension("toml.tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| "failed to create temporary preferences file")?;
    let result = file.write_all(source.as_bytes()).and_then(|()| file.sync_all());
    drop(file);
    let result = result.and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
        return Err("failed to write preferences file".into());
    }
    Ok(())
}

fn merge(changes: &mut BTreeMap<u64, Change>, updates: BTreeMap<u64, Change>) {
    changes.extend(updates);
}

fn persist(path: &Path, changes: &BTreeMap<u64, Change>) -> BatchResult {
    let current = match read(path) {
        Ok(saved) => saved,
        Err(error) => {
            return BatchResult {
                snapshot: None,
                changes: changes.iter().map(|(id, change)| (*id, *change, Err(error.clone()))).collect(),
            };
        }
    };
    let mut merged = current.clone();
    for (id, change) in changes {
        if current.knock.get(&id.to_string()).copied().unwrap_or_default() != change.value {
            merged.knock.insert(id.to_string(), change.value);
        }
    }
    let outcome = if merged.knock == current.knock { Ok(()) } else { write(path, &merged) };
    let snapshot = if outcome.is_ok() { merged } else { current };
    BatchResult {
        snapshot: Some(snapshot),
        changes: changes.iter().map(|(id, change)| (*id, *change, outcome.clone())).collect(),
    }
}

fn run_worker(path: PathBuf, requests: Receiver<Request>, events: Sender<WorkerEvent>) {
    let initial = read(&path);
    let _ = events.send(WorkerEvent::Loaded(initial));
    let mut pending = BTreeMap::new();
    let mut deferred = None;
    loop {
        let request = match deferred.take() {
            Some(request) => request,
            None => match requests.recv() {
                Ok(request) => request,
                Err(_) => break,
            },
        };
        match request {
            Request::Save(updates) => {
                merge(&mut pending, updates);
                loop {
                    match requests.try_recv() {
                        Ok(Request::Save(updates)) => merge(&mut pending, updates),
                        Ok(request) => {
                            deferred = Some(request);
                            break;
                        }
                        Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                    }
                }
                if !pending.is_empty() {
                    let result = persist(&path, &pending);
                    if result.changes.iter().all(|(_, _, result)| result.is_ok()) {
                        pending.clear();
                    }
                    let _ = events.send(WorkerEvent::Batch(result));
                }
            }
            Request::Drain(updates, done) => {
                merge(&mut pending, updates);
                let mut result = Ok(());
                if !pending.is_empty() {
                    let batch = persist(&path, &pending);
                    let failed = batch.changes.iter().find_map(|(_, _, result)| result.as_ref().err().cloned());
                    if let Some(error) = failed {
                        result = Err(error);
                    } else {
                        pending.clear();
                    }
                    let _ = events.send(WorkerEvent::Batch(batch));
                }
                if !pending.is_empty() && result.is_ok() {
                    result = Err("unsaved player preferences remain queued for retry".into());
                }
                let _ = done.send(result);
                break;
            }
        }
    }
}
