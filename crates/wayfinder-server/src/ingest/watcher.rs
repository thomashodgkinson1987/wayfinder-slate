//! Debounced filesystem watcher for the machine-local save folder.

use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use sqlx::SqlitePool;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::config::{Config, Mode};
use crate::ingest;
use crate::saves::MapObjects;

const QUIET_PERIOD: Duration = Duration::from_millis(750);
const MAX_DEBOUNCE: Duration = Duration::from_secs(2);

/// Handle to the background watcher. Dropping it stops the task on process
/// exit; prefer [`SaveWatcher::shutdown`] for graceful stops.
pub struct SaveWatcher {
    shutdown: Option<watch::Sender<bool>>,
    handle: Option<JoinHandle<()>>,
}

impl SaveWatcher {
    pub fn spawn(config: Config, pool: SqlitePool, objects: &'static MapObjects) -> Self {
        if !config.watch_saves || config.mode == Mode::Hub {
            return Self::disabled();
        }
        let Some(root) = config.save_root() else {
            return Self::disabled();
        };

        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let mut watcher =
            match notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                match result {
                    Ok(event) => {
                        let _ = event_tx.send(event);
                    }
                    Err(error) => tracing::warn!(%error, "save watcher error"),
                }
            }) {
                Ok(watcher) => watcher,
                Err(error) => {
                    tracing::warn!(%error, "could not create the save watcher");
                    return Self::disabled();
                }
            };

        if let Err(error) = watcher.watch(&root, RecursiveMode::Recursive) {
            tracing::warn!(path = %root.display(), %error, "could not watch the save folder");
            return Self::disabled();
        }

        tracing::info!(path = %root.display(), "watching the save folder");

        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let handle = tokio::spawn(async move {
            // The watcher must stay alive for events to keep flowing.
            let _watcher = watcher;

            loop {
                tokio::select! {
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() || *shutdown_rx.borrow() {
                            break;
                        }
                    }
                    event = event_rx.recv() => {
                        if event.is_none() {
                            break;
                        }
                        debounce(&mut event_rx).await;
                        let report = ingest::scan_machine_slots(&config, &pool, objects).await;
                        tracing::debug!(
                            slots = report.slots,
                            ingested = report.ingested,
                            unchanged = report.unchanged,
                            errors = report.errors,
                            "save folder scan complete"
                        );
                    }
                }
            }
        });

        Self {
            shutdown: Some(shutdown_tx),
            handle: Some(handle),
        }
    }

    pub async fn shutdown(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(true);
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }

    fn disabled() -> Self {
        Self {
            shutdown: None,
            handle: None,
        }
    }
}

impl Drop for SaveWatcher {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(true);
        }
    }
}

/// Wait until no event has arrived for [`QUIET_PERIOD`], bounded by
/// [`MAX_DEBOUNCE`] from the first event.
async fn debounce(events: &mut mpsc::UnboundedReceiver<notify::Event>) {
    let first = Instant::now();
    let mut last = first;

    loop {
        let quiet = last.elapsed();
        let total = first.elapsed();
        if debounce_ready(quiet, total) {
            break;
        }

        let wait = QUIET_PERIOD
            .saturating_sub(quiet)
            .min(MAX_DEBOUNCE.saturating_sub(total))
            .max(Duration::from_millis(10));

        match tokio::time::timeout(wait, events.recv()).await {
            Ok(Some(_event)) => last = Instant::now(),
            Ok(None) => break,
            Err(_) => {}
        }
    }
}

/// Pure debounce predicate kept separate for unit tests.
pub(crate) fn debounce_ready(quiet: Duration, total: Duration) -> bool {
    quiet >= QUIET_PERIOD || total >= MAX_DEBOUNCE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_needs_quiet_or_the_hard_cap() {
        assert!(!debounce_ready(
            Duration::from_millis(100),
            Duration::from_millis(100)
        ));
        assert!(debounce_ready(QUIET_PERIOD, Duration::from_millis(800)));
        assert!(debounce_ready(Duration::from_millis(0), MAX_DEBOUNCE));
        assert!(debounce_ready(QUIET_PERIOD, MAX_DEBOUNCE));
    }
}
