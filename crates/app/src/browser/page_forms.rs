//! Native pages that work like forms: the Account page (name, email, what to sync) and the
//! Welcome page's "Keep in sync" switches. What is typed is kept until it is sent.

use crate::browser::Browser;
use crate::native::{self, Step};

impl Browser {
    pub(crate) fn form_step(&mut self, step: Step) {
        let (path, body, done) = match step {
            Step::Input { id, value } => {
                self.native.inputs.insert(id, value);
                return;
            }
            Step::SyncType { key, on } => {
                self.native.inputs.insert(format!("type:{key}"), if on { "1" } else { "0" }.into());
                // Signed in, the choice applies at once; signed out it waits for the form.
                if !self.native.data["signedIn"].as_bool().unwrap_or(false) {
                    self.draw_native();
                    return;
                }
                ("account/types", self.chosen_types(), "Saved.")
            }
            Step::SignIn => {
                self.native.toast = "Finish signing in on Google's page in your browser…".into();
                self.draw_native();
                ("account/google", self.chosen_types(), "Signed in with Google. Sync is on.")
            }
            Step::Keep { id, on } => {
                let keep: Vec<&str> = self.native.data["browsers"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|b| {
                        let bid = b["id"].as_str()?;
                        let kept = if bid == id { on } else { b["keep"].as_bool().unwrap_or(false) };
                        kept.then_some(bid)
                    })
                    .collect();
                let done = if keep.is_empty() { "Not keeping any browser in sync." } else { "Kept in sync." };
                ("browsers/keep", keep.join(","), done)
            }
            _ => return,
        };
        let reply = self.reply_to(path, done);
        self.api(path, "", body, reply);
    }

    /// The sync types chosen on the Account page, as the API takes them ("bookmarks,history").
    fn chosen_types(&self) -> String {
        native::TYPES
            .iter()
            .filter(|(k, _)| native::type_on(&self.native.data, &self.native.inputs, k))
            .map(|(k, _)| *k)
            .collect::<Vec<_>>()
            .join(",")
    }
}
