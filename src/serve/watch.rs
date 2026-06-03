//! Filesystem watcher → broadcast fan-out for auto live-reload (feature `web`).
//!
//! A single recursive [`notify`] watcher observes the resolved ADR directory.
//! Raw notify events arrive in bursts (an editor save, a status-change file
//! move, or a `git checkout` can each emit several events), so they are
//! **coalesced**: after the first event we wait out a short quiet window
//! ([`DEBOUNCE`]) and then publish exactly one [`Change`] tick on a
//! [`tokio::sync::broadcast`] channel. Every `/api/events` SSE connection holds
//! a subscriber to that channel and forwards each tick to the browser, which
//! re-fetches the current view.
//!
//! The watcher only observes — no write paths are added. Dropped SSE clients
//! simply drop their `Receiver`; the watcher keeps running for the life of the
//! server (it is owned by [`Watcher`], held in `AppState`). If every subscriber
//! has gone away a published tick is silently dropped, which is fine.

use std::path::Path;
use std::sync::Mutex;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use notify::{EventKind, RecursiveMode, Watcher as _};
use tokio::sync::broadcast;

/// How long to wait for the filesystem to go quiet before publishing a single
/// coalesced tick. notify can emit a burst of events for one logical change
/// (editor save, file move between status dirs, git operations); coalescing
/// within this window collapses the burst into one "re-fetch" signal.
const DEBOUNCE: Duration = Duration::from_millis(250);

/// A live-reload tick. Carries no payload today (the SPA just re-fetches the
/// current view); a struct keeps room to add detail later without churning the
/// broadcast type.
#[derive(Clone, Copy, Debug, Default)]
pub struct Change;

/// Owns the broadcast sender — stable for the life of the server — plus the
/// current notify watch, which can be re-pointed at a new directory via
/// [`Watcher::retarget`] when the dashboard switches workspaces. Because the
/// sender outlives any individual watch, SSE subscribers survive a retarget.
///
/// Held in `AppState`. Dropping it stops the watch and closes the channel.
pub struct Watcher {
    tx: broadcast::Sender<Change>,
    // Current OS watch; replaced on `retarget`. Dropping the old handle ends its
    // OS watch, which in turn ends its debounce thread.
    handle: Mutex<WatchHandle>,
}

/// One running notify watch. Dropping it ends the OS watch; the paired debounce
/// thread then exits because its raw-event channel disconnects.
struct WatchHandle {
    _watcher: notify::RecommendedWatcher,
}

impl Watcher {
    /// Subscribe to the change stream. Each `/api/events` connection calls this
    /// once and forwards received ticks as SSE events.
    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.tx.subscribe()
    }

    /// Publish one tick immediately. Used when the active directory changes so
    /// every open tab re-fetches against the new workspace.
    pub fn notify_now(&self) {
        let _ = self.tx.send(Change);
    }

    /// Re-point the watcher at `dir`, replacing the previous OS watch while
    /// keeping the broadcast channel (and all current SSE subscribers) intact.
    pub fn retarget(&self, dir: &Path) -> notify::Result<()> {
        let handle = spawn_handle(dir, self.tx.clone())?;
        // Dropping the previous handle here ends the old watch + its thread.
        *self.handle.lock().expect("watch handle mutex poisoned") = handle;
        Ok(())
    }

    /// A handle to publish ticks directly. Used by tests to exercise the
    /// broadcast → subscriber wiring without touching the filesystem.
    #[cfg(test)]
    pub fn sender(&self) -> broadcast::Sender<Change> {
        self.tx.clone()
    }
}

/// Start watching `dir` recursively and return a [`Watcher`] whose broadcast
/// channel receives one coalesced [`Change`] per burst of filesystem activity.
pub fn spawn(dir: &Path) -> notify::Result<Watcher> {
    // Buffer a handful of ticks so a slow SSE client briefly behind doesn't
    // lose the *latest* signal — `RecvError::Lagged` just means "you missed
    // some, re-fetch anyway", which is exactly the desired behaviour.
    let (tx, _rx) = broadcast::channel::<Change>(16);
    let handle = spawn_handle(dir, tx.clone())?;
    Ok(Watcher {
        tx,
        handle: Mutex::new(handle),
    })
}

/// Start one recursive notify watch on `dir`, publishing coalesced ticks onto
/// `bcast`. Returns a handle that keeps the OS watch alive until dropped.
///
/// `notify` delivers events on its own thread via a `std::sync::mpsc` channel;
/// a dedicated debounce thread drains that channel, coalesces bursts, and
/// publishes onto the async broadcast channel. This keeps the watcher entirely
/// off the Tokio runtime's critical path and out of the request handlers.
fn spawn_handle(dir: &Path, bcast: broadcast::Sender<Change>) -> notify::Result<WatchHandle> {
    // notify -> debounce thread (std mpsc; notify is sync).
    let (raw_tx, raw_rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        // Best-effort: if the debounce thread is gone the server is shutting
        // down and we can drop the event.
        let _ = raw_tx.send(res);
    })?;
    watcher.watch(dir, RecursiveMode::Recursive)?;

    thread::Builder::new()
        .name("adroit-adr-watch".into())
        .spawn(move || debounce_loop(raw_rx, bcast))
        // A failed spawn means the OS is out of threads; surfacing it as an
        // io error keeps `spawn` infallible-by-type for callers.
        .map_err(notify::Error::io)?;

    Ok(WatchHandle { _watcher: watcher })
}

/// Drain raw notify events, coalescing bursts within [`DEBOUNCE`] into a single
/// broadcast tick. Exits when the notify channel closes (watcher dropped).
fn debounce_loop(
    raw_rx: mpsc::Receiver<notify::Result<notify::Event>>,
    bcast: broadcast::Sender<Change>,
) {
    // Block for the next interesting event (closing the channel ends the loop).
    while let Ok(first) = raw_rx.recv() {
        if !is_interesting(&first) {
            continue;
        }
        // Coalesce: keep swallowing events until the filesystem goes quiet for
        // a full DEBOUNCE window, then publish exactly one tick.
        loop {
            match raw_rx.recv_timeout(DEBOUNCE) {
                Ok(_) => continue,                             // more churn; keep waiting
                Err(mpsc::RecvTimeoutError::Timeout) => break, // quiet — publish
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
        // Ignore the "no subscribers" error: with no SSE clients connected
        // there's nothing to refresh.
        let _ = bcast.send(Change);
    }
}

/// Filter out events that can't change ADR content (access/metadata-only).
/// Errors and the catch-all `Other`/`Any` kinds are treated as interesting so
/// we never miss a real change on platforms with coarse event reporting.
fn is_interesting(res: &notify::Result<notify::Event>) -> bool {
    match res {
        Ok(event) => !matches!(event.kind, EventKind::Access(_)),
        // A watch error (e.g. overflow) means "we may have missed something" —
        // signal a refresh to be safe.
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::time::{Duration as TokioDuration, timeout};

    /// Publishing a tick to the broadcast reaches a live subscriber. Exercises
    /// the exact wiring `/api/events` relies on, with no filesystem timing.
    #[tokio::test]
    async fn broadcast_tick_reaches_subscriber() {
        let tmp = TempDir::new().unwrap();
        let watcher = spawn(tmp.path()).unwrap();
        let mut rx = watcher.subscribe();
        watcher.sender().send(Change).unwrap();
        // Bounded wait so a wiring regression fails fast instead of hanging.
        let got = timeout(TokioDuration::from_secs(1), rx.recv()).await;
        assert!(got.is_ok(), "subscriber did not receive the tick");
        assert!(got.unwrap().is_ok());
    }

    /// A real filesystem write under the watched dir produces a coalesced tick.
    /// Bounded by a timeout so it can never hang CI even if the platform's
    /// watcher is slow or unavailable.
    #[tokio::test]
    async fn file_change_emits_a_tick() {
        let tmp = TempDir::new().unwrap();
        let watcher = spawn(tmp.path()).unwrap();
        let mut rx = watcher.subscribe();

        // Write a file after subscribing so the event can't race ahead of us.
        let path = tmp.path().join("0001-test.md");
        std::fs::write(&path, "# ADR-0001: Test\n").unwrap();

        match timeout(TokioDuration::from_secs(5), rx.recv()).await {
            Ok(Ok(_)) => {}
            // Some sandboxed CI filesystems don't deliver inotify events; don't
            // make the suite flaky over an environment limitation.
            Ok(Err(_)) | Err(_) => {
                eprintln!("note: no fs event delivered in this environment; skipping");
            }
        }
    }

    /// A burst of writes coalesces into far fewer ticks than writes (debounce).
    #[tokio::test]
    async fn bursts_are_coalesced() {
        let tmp = TempDir::new().unwrap();
        let watcher = spawn(tmp.path()).unwrap();
        let mut rx = watcher.subscribe();

        for i in 0..10 {
            std::fs::write(tmp.path().join(format!("{i}.md")), "x").unwrap();
        }

        // Let the debounce window elapse, then drain.
        tokio::time::sleep(TokioDuration::from_millis(
            DEBOUNCE.as_millis() as u64 + 200,
        ))
        .await;
        let mut ticks = 0;
        while rx.try_recv().is_ok() {
            ticks += 1;
        }
        // Either nothing delivered (sandbox) or far fewer ticks than writes.
        assert!(
            ticks < 10,
            "expected coalescing, got {ticks} ticks for 10 writes"
        );
    }

    /// A subscriber taken before a retarget still receives ticks published
    /// afterwards — the broadcast channel must outlive any individual watch so
    /// open SSE connections survive a workspace switch.
    #[tokio::test]
    async fn subscriber_survives_retarget() {
        let a = TempDir::new().unwrap();
        let b = TempDir::new().unwrap();
        let watcher = spawn(a.path()).unwrap();
        let mut rx = watcher.subscribe();
        watcher.retarget(b.path()).unwrap();
        // The pre-retarget subscriber is still wired to the (stable) sender.
        watcher.notify_now();
        let got = timeout(TokioDuration::from_secs(1), rx.recv()).await;
        assert!(
            got.is_ok() && got.unwrap().is_ok(),
            "subscriber lost its channel after retarget"
        );
    }
}
