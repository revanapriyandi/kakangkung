//! A tab group's right-click menu: rename, colour, ungroup, close.

use serde_json::json;

use crate::browser::Browser;

/// Names of `ui::GROUP_COLORS`, in the same order.
const COLOR_NAMES: [&str; 8] = ["Blue", "Red", "Yellow", "Green", "Pink", "Purple", "Cyan", "Orange"];

impl Browser {
    /// Show the menu of group `gid` at `x, y` (DIPs of the tab strip); the chosen command.
    pub(crate) fn show_group_menu(&mut self, gid: u32, x: f64, y: f64) -> Option<String> {
        let g = self.groups.iter().find(|g| g.id == gid)?.clone();
        self.menu_at = (x, y);
        let colors: Vec<_> = COLOR_NAMES
            .iter()
            .zip(ui::GROUP_COLORS)
            .enumerate()
            .map(|(k, (name, hex))| json!({ "id": format!("group:color:{gid}:{k}"), "label": name, "key": "", "checked": hex == g.color }))
            .collect();
        let rows = vec![
            json!({ "id": "", "label": g.name, "key": "" }),
            json!({ "sep": true }),
            json!({ "id": format!("group:rename:{gid}"), "label": "Rename group…", "key": "" }),
            json!({ "label": "Colour", "sub": colors }),
            json!({ "id": format!("group:collapse:{gid}"), "label": if g.collapsed { "Expand group" } else { "Collapse group" }, "key": "" }),
            json!({ "sep": true }),
            json!({ "id": format!("group:ungroup:{gid}"), "label": "Ungroup", "key": "" }),
            json!({ "id": format!("group:close:{gid}"), "label": "Close group", "key": "" }),
        ];
        self.native_popup(rows, x, y, false)
    }

    /// `rename:<gid>`, `color:<gid>:<index>`, `collapse:<gid>`, `ungroup:<gid>`, `close:<gid>`.
    pub(crate) fn group_action(&mut self, what: &str) {
        let mut f = what.split(':');
        let (action, gid) = (f.next().unwrap_or(""), f.next().and_then(|g| g.parse::<u32>().ok()).unwrap_or(0));
        let Some(g) = self.groups.iter().position(|g| g.id == gid) else { return };
        match action {
            "rename" => {
                // A small field just under the label, holding the current name.
                let (x, y) = self.menu_at;
                let name = self.groups[g].name.clone();
                self.overlay_open = true;
                self.overlay_kind = "input".into();
                let dark = self.dark_ui();
                self.popup.show_input(&format!("grouprename:{gid}"), x as f32, (platform::frame::STRIP_H - 4.0).max(y as f32 + 12.0), 260.0, &name, "Name this group", dark);
            }
            "color" => {
                if let Some(c) = f.next().and_then(|k| k.parse::<usize>().ok()).and_then(|k| ui::GROUP_COLORS.get(k)) {
                    self.groups[g].color = c;
                }
            }
            "collapse" => self.groups[g].collapsed = !self.groups[g].collapsed,
            "ungroup" => {
                self.tabs.iter_mut().filter(|t| t.view.group == gid).for_each(|t| t.view.group = 0);
                self.prune_groups();
            }
            "close" => {
                let ids: Vec<u64> = self.tabs.iter().filter(|t| t.view.group == gid).map(|t| t.view.id).collect();
                for id in ids {
                    self.close_tab(id);
                }
            }
            _ => {}
        }
        self.render();
    }
}

#[cfg(test)]
mod tests {
    use super::COLOR_NAMES;

    #[test]
    fn every_group_colour_has_a_name() {
        assert_eq!(COLOR_NAMES.len(), ui::GROUP_COLORS.len());
    }
}
