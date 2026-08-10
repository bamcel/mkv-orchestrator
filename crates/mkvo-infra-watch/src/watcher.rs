use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::{
    ReconcileChangeKind, Snapshot, diff_snapshots, is_supported_media_path, snapshot_roots,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchMode {
    Auto,
    Native,
    Polling,
}

#[derive(Debug, Clone)]
pub struct WatchOptions {
    pub roots: Vec<PathBuf>,
    pub mode: WatchMode,
    pub debounce: Duration,
    pub poll_interval: Duration,
    pub media_only: bool,
    pub channel_capacity: usize,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            mode: WatchMode::Auto,
            debounce: Duration::from_secs(3),
            poll_interval: Duration::from_secs(30),
            media_only: true,
            channel_capacity: 256,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchEventKind {
    Created,
    Modified,
    Removed,
    Renamed,
    RescanRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchEvent {
    pub kind: WatchEventKind,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Error)]
pub enum WatchStartError {
    #[error("at least one watch root is required")]
    NoRoots,
    #[error("watch root is not a directory: `{0}`")]
    InvalidRoot(PathBuf),
    #[error("native watcher failed: {0}")]
    Notify(#[from] notify::Error),
    #[error("initial polling snapshot failed: {0}")]
    Snapshot(#[from] crate::SnapshotError),
}

pub struct WatchHandle {
    receiver: mpsc::Receiver<WatchEvent>,
    native: Option<RecommendedWatcher>,
    bridge_task: Option<JoinHandle<()>>,
    cancellation: CancellationToken,
    active_mode: WatchMode,
}

impl std::fmt::Debug for WatchHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WatchHandle")
            .field("active_mode", &self.active_mode)
            .finish_non_exhaustive()
    }
}

impl WatchHandle {
    pub fn start(options: WatchOptions) -> Result<Self, WatchStartError> {
        validate_roots(&options.roots)?;
        match options.mode {
            WatchMode::Native => Self::start_native(options),
            WatchMode::Polling => Self::start_polling(options),
            WatchMode::Auto => match Self::start_native(options.clone()) {
                Ok(handle) => Ok(handle),
                Err(error) => {
                    tracing::warn!(%error, "native filesystem watch unavailable; using polling reconciliation");
                    Self::start_polling(options)
                }
            },
        }
    }

    pub const fn active_mode(&self) -> WatchMode {
        self.active_mode
    }

    pub async fn recv(&mut self) -> Option<WatchEvent> {
        self.receiver.recv().await
    }

    pub fn try_recv(&mut self) -> Result<WatchEvent, mpsc::error::TryRecvError> {
        self.receiver.try_recv()
    }

    pub fn stop(&self) {
        self.cancellation.cancel();
    }

    fn start_native(options: WatchOptions) -> Result<Self, WatchStartError> {
        let (raw_sender, raw_receiver) = mpsc::channel(options.channel_capacity.max(1));
        let (sender, receiver) = mpsc::channel(options.channel_capacity.max(1));
        let callback_sender = raw_sender.clone();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = callback_sender.try_send(event);
        })?;
        for root in &options.roots {
            watcher.watch(root, RecursiveMode::Recursive)?;
        }
        let cancellation = CancellationToken::new();
        let bridge_task = tokio::spawn(debounce_native_events(
            raw_receiver,
            sender,
            cancellation.clone(),
            options.debounce,
            options.media_only,
        ));
        Ok(Self {
            receiver,
            native: Some(watcher),
            bridge_task: Some(bridge_task),
            cancellation,
            active_mode: WatchMode::Native,
        })
    }

    fn start_polling(options: WatchOptions) -> Result<Self, WatchStartError> {
        let initial = snapshot_roots(&options.roots, options.media_only)?;
        let (sender, receiver) = mpsc::channel(options.channel_capacity.max(1));
        let cancellation = CancellationToken::new();
        let bridge_task = tokio::spawn(poll_loop(options, initial, sender, cancellation.clone()));
        Ok(Self {
            receiver,
            native: None,
            bridge_task: Some(bridge_task),
            cancellation,
            active_mode: WatchMode::Polling,
        })
    }
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.native.take();
        if let Some(task) = self.bridge_task.take() {
            task.abort();
        }
    }
}

fn validate_roots(roots: &[PathBuf]) -> Result<(), WatchStartError> {
    if roots.is_empty() {
        return Err(WatchStartError::NoRoots);
    }
    if let Some(root) = roots.iter().find(|root| !root.is_dir()) {
        return Err(WatchStartError::InvalidRoot(root.clone()));
    }
    Ok(())
}

async fn debounce_native_events(
    mut raw_receiver: mpsc::Receiver<notify::Result<Event>>,
    sender: mpsc::Sender<WatchEvent>,
    cancellation: CancellationToken,
    debounce: Duration,
    media_only: bool,
) {
    let tick_duration = debounce
        .min(Duration::from_secs(1))
        .max(Duration::from_millis(50));
    let mut tick = tokio::time::interval(tick_duration);
    let mut pending = BTreeMap::<PathBuf, (WatchEventKind, Instant)>::new();
    loop {
        tokio::select! {
            () = cancellation.cancelled() => break,
            raw = raw_receiver.recv() => {
                let Some(raw) = raw else { break; };
                match raw {
                    Ok(event) => queue_notify_event(event, media_only, &mut pending),
                    Err(error) => {
                        tracing::warn!(%error, "filesystem watcher reported an error; reconciliation required");
                        if sender.send(WatchEvent { kind: WatchEventKind::RescanRequired, paths: Vec::new() }).await.is_err() {
                            break;
                        }
                    }
                }
            }
            _ = tick.tick() => {
                let now = Instant::now();
                let ready: Vec<_> = pending
                    .iter()
                    .filter(|(_, (_, at))| now.duration_since(*at) >= debounce)
                    .map(|(path, (kind, _))| (path.clone(), *kind))
                    .collect();
                for (path, kind) in ready {
                    pending.remove(&path);
                    if sender.send(WatchEvent { kind, paths: vec![path] }).await.is_err() {
                        return;
                    }
                }
            }
        }
    }
}

fn queue_notify_event(
    event: Event,
    media_only: bool,
    pending: &mut BTreeMap<PathBuf, (WatchEventKind, Instant)>,
) {
    let kind = match event.kind {
        EventKind::Create(_) => WatchEventKind::Created,
        EventKind::Modify(notify::event::ModifyKind::Name(_)) => WatchEventKind::Renamed,
        EventKind::Modify(_) => WatchEventKind::Modified,
        EventKind::Remove(_) => WatchEventKind::Removed,
        EventKind::Any | EventKind::Other | EventKind::Access(_) => return,
    };
    let at = Instant::now();
    for path in event.paths {
        if media_only && path.extension().is_some() && !is_supported_media_path(&path) {
            continue;
        }
        pending.insert(path, (kind, at));
    }
}

async fn poll_loop(
    options: WatchOptions,
    mut previous: Snapshot,
    sender: mpsc::Sender<WatchEvent>,
    cancellation: CancellationToken,
) {
    let mut interval = tokio::time::interval(options.poll_interval.max(Duration::from_secs(1)));
    interval.tick().await;
    loop {
        tokio::select! {
            () = cancellation.cancelled() => break,
            _ = interval.tick() => {
                let roots = options.roots.clone();
                let media_only = options.media_only;
                let snapshot = tokio::task::spawn_blocking(move || snapshot_roots(&roots, media_only)).await;
                match snapshot {
                    Ok(Ok(current)) => {
                        for change in diff_snapshots(&previous, &current) {
                            let kind = match change.kind {
                                ReconcileChangeKind::Created => WatchEventKind::Created,
                                ReconcileChangeKind::Modified => WatchEventKind::Modified,
                                ReconcileChangeKind::Removed => WatchEventKind::Removed,
                            };
                            if sender.send(WatchEvent { kind, paths: vec![change.path] }).await.is_err() {
                                return;
                            }
                        }
                        previous = current;
                    }
                    Ok(Err(error)) => tracing::warn!(%error, "polling filesystem reconciliation failed"),
                    Err(error) => tracing::warn!(%error, "polling filesystem task failed"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_roots() {
        assert!(matches!(
            WatchHandle::start(WatchOptions::default()),
            Err(WatchStartError::NoRoots)
        ));
    }

    #[test]
    fn event_filter_keeps_media_and_directories() {
        let mut pending = BTreeMap::new();
        queue_notify_event(
            Event {
                kind: EventKind::Create(notify::event::CreateKind::File),
                paths: vec![PathBuf::from("cover.jpg"), PathBuf::from("episode.mkv")],
                attrs: notify::event::EventAttributes::new(),
            },
            true,
            &mut pending,
        );
        assert_eq!(pending.len(), 1);
        assert!(pending.contains_key(&PathBuf::from("episode.mkv")));
    }
}
