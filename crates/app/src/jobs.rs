//! The storage thread: owns the database; every job runs there, never on the UI thread.

use std::path::PathBuf;
use std::sync::mpsc;

use storage::Store;

/// Work for the storage thread: runs off the UI thread with the open database (or None if it
/// could not be opened).
pub(crate) type Job = Box<dyn FnOnce(Option<&Store>) + Send>;

/// Storage thread: owns the database; every job runs here, never on the UI thread.
pub(crate) fn spawn_storage(db: PathBuf) -> (mpsc::Sender<Job>, std::thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<Job>();
    let handle = std::thread::spawn(move || {
        let store = Store::open(&db).map_err(|e| eprintln!("history database: {e}")).ok();
        for job in rx {
            job(store.as_ref());
        }
    });
    (tx, handle)
}
