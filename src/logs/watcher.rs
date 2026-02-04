use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use tokio::sync::mpsc as tokio_mpsc;

pub enum WatcherEvent {
    FileModified(PathBuf),
    Error(String),
}

pub struct LogWatcher {
    _watcher: notify_debouncer_mini::Debouncer<RecommendedWatcher>,
    event_rx: tokio_mpsc::UnboundedReceiver<WatcherEvent>,
}

impl LogWatcher {
    pub fn new(path: PathBuf) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let (event_tx, event_rx) = tokio_mpsc::unbounded_channel();

        let mut debouncer = new_debouncer(Duration::from_millis(100), tx)?;

        debouncer
            .watcher()
            .watch(&path, RecursiveMode::NonRecursive)?;

        // Spawn a thread to convert sync events to async
        let watch_path = path.clone();
        std::thread::spawn(move || {
            while let Ok(events) = rx.recv() {
                match events {
                    Ok(events) => {
                        for event in events {
                            if event.kind == DebouncedEventKind::Any
                                && (event.path == watch_path || event.path.starts_with(&watch_path))
                            {
                                let _ = event_tx.send(WatcherEvent::FileModified(event.path));
                            }
                        }
                    }
                    Err(e) => {
                        let _ = event_tx.send(WatcherEvent::Error(e.to_string()));
                    }
                }
            }
        });

        Ok(Self {
            _watcher: debouncer,
            event_rx,
        })
    }

    pub async fn next_event(&mut self) -> Option<WatcherEvent> {
        self.event_rx.recv().await
    }
}

pub struct SessionWatcher {
    watcher: Option<LogWatcher>,
    current_path: Option<PathBuf>,
    file_position: u64,
}

impl SessionWatcher {
    pub fn new() -> Self {
        Self {
            watcher: None,
            current_path: None,
            file_position: 0,
        }
    }

    pub fn watch(&mut self, path: PathBuf) -> Result<()> {
        // Reset position for new file
        self.file_position = 0;
        self.current_path = Some(path.clone());
        self.watcher = Some(LogWatcher::new(path)?);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.watcher = None;
        self.current_path = None;
        self.file_position = 0;
    }

    pub async fn next_event(&mut self) -> Option<WatcherEvent> {
        if let Some(ref mut watcher) = self.watcher {
            watcher.next_event().await
        } else {
            // If no watcher, just pend forever
            std::future::pending().await
        }
    }

    pub fn file_position(&self) -> u64 {
        self.file_position
    }

    pub fn set_file_position(&mut self, pos: u64) {
        self.file_position = pos;
    }

    pub fn current_path(&self) -> Option<&PathBuf> {
        self.current_path.as_ref()
    }
}

impl Default for SessionWatcher {
    fn default() -> Self {
        Self::new()
    }
}
