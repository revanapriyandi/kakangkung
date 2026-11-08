//! Downloads, Extensions and Task manager pages, described from their data: the downloads of
//! this run, the `extensions` answer and the `memory` answer.

use platform::page::{ButtonStyle, Control, PageDoc, Row, Section};
use serde_json::Value as Json;

use super::build::{buttons, empty, initial, nav, section};
use super::when::{clock, day_title, local};

/// "3.4 MB".
pub(crate) fn size(bytes: i64) -> String {
    let b = bytes.max(0) as f64;
    match b {
        b if b >= 1_073_741_824.0 => format!("{:.1} GB", b / 1_073_741_824.0),
        b if b >= 1_048_576.0 => format!("{:.1} MB", b / 1_048_576.0),
        b if b >= 1024.0 => format!("{:.0} KB", b / 1024.0),
        b => format!("{b:.0} B"),
    }
}

/// The Downloads page: downloads of this run and kept ones, grouped by day, newest first.
pub(crate) fn downloads_doc(items: &Json) -> PageDoc {
    let now = storage::now_ms();
    let (today, yesterday) = (local(now), local(now - 86_400_000));
    let mut sections: Vec<Section> = vec![];
    for d in items.as_array().into_iter().flatten() {
        let id = match &d["id"] {
            Json::String(s) => s.clone(),
            other => other.to_string(),
        };
        let path = d["path"].as_str().unwrap_or("");
        let url = d["url"].as_str().unwrap_or("");
        let name = path.rsplit(['\\', '/']).next().filter(|n| !n.is_empty()).unwrap_or(url);
        let (got, total) = (d["received"].as_i64().unwrap_or(0), d["total"].as_i64().unwrap_or(-1));
        let of = if total > 0 { format!("{} of {}", size(got), size(total)) } else { size(got) };
        let pct = if total > 0 { format!(", {}%", got * 100 / total) } else { String::new() };
        let b = |what: &str, label: &'static str, style| (format!("dl-{what}:{id}"), label, style);
        let n = ButtonStyle::Normal;
        let (state, control) = match d["state"].as_str().unwrap_or("") {
            "InProgress" => (format!("{of}{pct}"), buttons(&[b("pause", "Pause", n), b("cancel", "Cancel", ButtonStyle::Danger)])),
            "Paused" => (format!("Paused · {of}{pct}"), buttons(&[b("resume", "Resume", ButtonStyle::Primary), b("cancel", "Cancel", ButtonStyle::Danger)])),
            "Done" => (size(got), buttons(&[b("open", "Open", n), b("show", "Show in folder", n), b("forget", "Remove", n)])),
            _ => ("Cancelled or failed".into(), buttons(&[b("forget", "Remove", n)])),
        };
        let t = local(d["started"].as_i64().unwrap_or(now));
        let title = day_title(t, today, yesterday);
        if sections.last().is_none_or(|s| s.title != title) {
            sections.push(section(&format!("day-{}-{}-{}", t.year, t.month, t.day), &title, vec![]));
        }
        let row = Row::new(name, &format!("{} · {} · {state}", clock(t), ui::host(url)), control);
        if let Some(card) = sections.last_mut().and_then(|s| s.cards.last_mut()) {
            card.push(row);
        }
    }
    if sections.is_empty() {
        sections.push(empty("none", "", "\u{E896}", "Files you download appear here", "Finished downloads stay listed until you remove them; the files stay in your downloads folder."));
    }
    PageDoc {
        title: "Downloads".into(),
        search: "Search downloads".into(),
        nav: nav(&[("open:downloads", "Downloads", "\u{E896}"), ("folder", "Open downloads folder", "\u{E838}"), ("clear", "Clear the list", "\u{E74D}")]),
        sections,
        nav_on: "open:downloads".into(),
        ..Default::default()
    }
}

/// The Extensions page for the `extensions` answer (a list, or an error).
pub(crate) fn extensions_doc(answer: &Json) -> PageDoc {
    let mut sections = vec![];
    match answer.as_array() {
        Some(list) if !list.is_empty() => {
            let cards = list
                .iter()
                .map(|x| {
                    let id = x["id"].as_str().unwrap_or("");
                    let name = x["name"].as_str().unwrap_or(id);
                    let on = x["enabled"].as_bool().unwrap_or(false);
                    vec![
                        Row::new(name, &format!("ID: {id}"), Control::Avatar { initial: initial(name) }),
                        Row::new(if on { "On" } else { "Off" }, "", Control::Toggle { id: format!("ext:{id}"), on }),
                        Row::new("Remove from Kakangkung", "", buttons(&[(format!("ext-remove:{id}"), "Remove", ButtonStyle::Danger)])),
                    ]
                })
                .collect();
            sections.push(Section { id: "installed".into(), title: format!("Installed ({})", list.len()), keywords: String::new(), cards });
        }
        Some(_) => sections.push(empty("installed", "Installed", "\u{EA86}", "No extensions yet", "Add one from the Chrome Web Store, load an unpacked one, or import one from another browser below.")),
        None => sections.push(empty("installed", "Installed", "\u{EA86}", "Extensions are not available right now", answer["error"].as_str().unwrap_or(""))),
    }
    sections.push(section("add", "Add an extension", vec![
        Row::new("Chrome Web Store", "Open an extension's page and choose Add to Kakangkung", Control::Link { id: "visit:https://chromewebstore.google.com/category/extensions".into() }),
        Row::new("Load unpacked", "The folder that contains manifest.json", buttons(&[("ext-load", "Load unpacked", ButtonStyle::Primary)])),
        Row::new("Import from a browser", "Chrome, Edge or Brave", Control::Link { id: "open:welcome#chrome".into() }),
    ]));
    sections.push(section("about", "", vec![Row::new(
        "How extensions run",
        "Extensions run in Kakangkung's tabs through Microsoft WebView2: content scripts, background work and blocking rules run. The extensions button in the toolbar opens an extension's own pop-up; it runs beside the page, so an extension that acts on \"the current tab\" may not see the page behind it.",
        Control::None,
    )]));
    PageDoc {
        title: "Extensions".into(),
        search: "Search extensions".into(),
        nav: nav(&[("open:extensions", "My extensions", "\u{EA86}"), ("open:settings", "Settings", "\u{E713}")]),
        sections,
        nav_on: "open:extensions".into(),
        ..Default::default()
    }
}

/// The Task manager page for the `memory` answer.
pub(crate) fn taskmanager_doc(m: &Json) -> PageDoc {
    let mut procs: Vec<&Json> = m["processes"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    procs.sort_by_key(|p| std::cmp::Reverse(p["ws"].as_i64().unwrap_or(0)));
    let total: i64 = procs.iter().map(|p| p["ws"].as_i64().unwrap_or(0)).sum();
    let commit: i64 = procs.iter().map(|p| p["commit"].as_i64().unwrap_or(0)).sum();
    let n = |k: &str| m[k].as_u64().unwrap_or(0);
    let overview = vec![
        Row::new("Memory in use", "Every Kakangkung process, counted like the Memory column of Windows Task Manager", Control::Value(size(total))),
        Row::new("Tabs", &format!("{} running · {} frozen · {} not loaded", n("running"), n("frozen"), n("unloaded")), Control::Value((n("running") + n("frozen") + n("unloaded")).to_string())),
        Row::new("Memory Saver", m["mode"].as_str().unwrap_or(""), Control::Link { id: "open:settings/performance".into() }),
    ];
    let mut rows: Vec<Row> = procs
        .iter()
        .map(|p| {
            let sub = format!("PID {} · commit {}", p["pid"], size(p["commit"].as_i64().unwrap_or(0)));
            Row::new(p["kind"].as_str().unwrap_or("Process"), &sub, Control::Value(size(p["ws"].as_i64().unwrap_or(0))))
        })
        .collect();
    rows.push(Row::new("Total", &format!("{} processes · commit {}", procs.len(), size(commit)), Control::Value(size(total))));
    PageDoc {
        title: "Task manager".into(),
        search: "Search processes".into(),
        nav: nav(&[("open:taskmanager", "Task manager", "\u{E9D9}"), ("open:settings/performance", "Performance settings", "\u{EC4A}")]),
        sections: vec![
            section("overview", "Overview", overview),
            section("processes", "Processes", rows),
            section("about", "", vec![Row::new(
                "About these numbers",
                "Memory is the private working set, the same number as the Memory column in Windows Task Manager. Renderer processes can be shared by several tabs of the same site, so memory is shown per process. Updated every 2 seconds.",
                Control::None,
            )]),
        ],
        nav_on: "open:taskmanager".into(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rows(doc: &PageDoc) -> Vec<&Row> {
        doc.sections.iter().flat_map(|s| s.cards.iter().flatten()).collect()
    }

    #[test]
    fn sizes_read_like_task_manager() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(3 * 1_048_576 + 400_000), "3.4 MB");
        assert_eq!(size(2_147_483_648), "2.0 GB");
    }

    #[test]
    fn downloads_offer_what_their_state_allows() {
        let items = json!([
            { "id": 2, "url": "https://a.example/f.zip", "path": "C:\\\\D\\\\f.zip", "received": 50, "total": 100, "state": "InProgress", "started": storage::now_ms() },
            { "id": 1, "url": "https://a.example/g.pdf", "path": "C:\\\\D\\\\g.pdf", "received": 10, "total": 10, "state": "Done", "started": storage::now_ms() },
        ]);
        let doc = downloads_doc(&items);
        let r = rows(&doc);
        assert_eq!(r[0].title, "f.zip");
        assert!(r[0].sub.ends_with("50%"));
        let ids = |row: &Row| match &row.control {
            Control::Buttons(b) => b.iter().map(|b| b.id.clone()).collect::<Vec<_>>(),
            _ => vec![],
        };
        assert_eq!(ids(r[0]), ["dl-pause:2", "dl-cancel:2"]);
        assert_eq!(ids(r[1]), ["dl-open:1", "dl-show:1", "dl-forget:1"]);
    }

    #[test]
    fn extensions_and_processes() {
        let doc = extensions_doc(&json!([{ "id": "abc", "name": "Blocker", "enabled": true }]));
        assert!(rows(&doc).iter().any(|r| matches!(&r.control, Control::Toggle { id, on: true } if id == "ext:abc")));
        let err = extensions_doc(&json!({ "error": "open a tab first" }));
        assert!(rows(&err).iter().any(|r| r.sub == "open a tab first"));
        let tm = taskmanager_doc(&json!({ "processes": [{ "pid": 1, "kind": "GPU", "ws": 1_048_576, "commit": 0 }, { "pid": 2, "kind": "Browser", "ws": 3_145_728, "commit": 0 }], "running": 1, "frozen": 0, "unloaded": 2, "mode": "Balanced" }));
        let procs = &tm.sections[1].cards[0];
        assert_eq!(procs[0].title, "Browser");
        assert_eq!(procs.last().map(|r| r.control.clone()), Some(Control::Value("4.0 MB".into())));
    }
}
