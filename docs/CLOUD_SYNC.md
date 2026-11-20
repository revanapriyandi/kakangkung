# Cloud sync foundation (B6)

Kakangkung does **not** speak Google Chrome Sync. It has its own sync:

1. **Folder sync** — a directory mirrored by OneDrive / Google Drive / Dropbox / etc.
   (`crates/storage/src/sync.rs`). Types: tabs, bookmarks, history, settings, extensions,
   and passwords when enabled (KB-21).
2. **Google Drive `appDataFolder`** — optional account-bound device files (see
   `docs/CHROME_PARITY.md` / Google sign-in docs).

This is the real foundation for “cloud sync”. Expanding encryption, conflict UX, and
cross-device password vault polish continues on this path — not a stub Chrome Sync clone.
