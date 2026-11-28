//! One in-process lock per directory, shared by every caller in the process.
//!
//! Loom's service owns its data dir, so a lock inside the process is enough; if two processes
//! share a data dir, git's own lock files refuse the second writer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use tokio::sync::OwnedMutexGuard;

type LockMap = Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>;

static LOCKS: LazyLock<LockMap> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Waits for the lock of `path` and holds it until the guard is dropped.
pub(crate) async fn lock(path: &Path) -> OwnedMutexGuard<()> {
    let mutex = {
        let mut map = LOCKS.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(map.entry(path.to_owned()).or_default())
    };
    mutex.lock_owned().await
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use super::lock;

    #[tokio::test]
    async fn a_second_caller_waits_for_the_first() {
        let path = Path::new("a-lock-test-path");
        let guard = lock(path).await;
        let waiting = tokio::time::timeout(Duration::from_millis(50), lock(path)).await;
        assert!(waiting.is_err(), "the lock was taken twice");
        drop(guard);
        assert!(
            tokio::time::timeout(Duration::from_millis(500), lock(path))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn different_paths_do_not_block_each_other() {
        let _one = lock(Path::new("lock-test-one")).await;
        let two =
            tokio::time::timeout(Duration::from_millis(500), lock(Path::new("lock-test-two")));
        assert!(two.await.is_ok());
    }
}
