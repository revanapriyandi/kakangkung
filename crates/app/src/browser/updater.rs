//! The browser's side of automatic updates (`crate::update`): checking on its own thread,
//! "Restart to update", and putting a downloaded update in when Kakangkung closes.

use crate::browser::Browser;
use crate::msg::Msg;
use crate::update::{self, Status};

impl Browser {
    /// Look for an update in the background (unless a check or download is under way).
    pub(crate) fn check_updates(&mut self) {
        if self.update.busy() || matches!(self.update, Status::Ready { .. }) {
            return;
        }
        self.update = Status::Checking;
        let (dir, p) = (self.update_dir.clone(), self.proxy.clone());
        std::thread::spawn(move || {
            let send = |s: Status| {
                let _ = p.send_event(Msg::Update(s));
            };
            send(update::run(&dir, send));
        });
    }

    /// A state from the update thread.
    pub(crate) fn on_update(&mut self, status: Status) {
        self.update = status;
        if self.native_page() == Some("settings") {
            self.refresh_native("settings");
        }
    }

    /// Start the installer of the downloaded update; it opens Kakangkung again with its tabs.
    /// True when Kakangkung should now close.
    pub(crate) fn restart_to_update(&mut self) -> bool {
        let Status::Ready { installer, .. } = &self.update else { return false };
        // The tabs come back after the update ("--restore-session").
        self.save_session(self.session());
        match update::install(installer, true) {
            Ok(()) => {
                self.update = Status::Idle;
                true
            }
            Err(e) => {
                self.update = Status::Failed(e);
                false
            }
        }
    }

    /// Kakangkung is closing: put a downloaded update in (silently), when updates are automatic.
    pub(crate) fn install_pending_update(&mut self) {
        if let (Status::Ready { installer, .. }, true) = (&self.update, self.settings.auto_update) {
            if let Err(e) = update::install(installer, false) {
                eprintln!("update: {e}");
            }
        }
    }
}
