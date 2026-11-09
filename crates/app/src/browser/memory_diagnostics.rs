use serde_json::{json, Value};
use super::Browser;

impl Browser {
    pub(crate) fn memory_snapshot(&self) -> Value {
        let visible = self.visible_with(self.active);
        let tabs: Vec<_> = self.tabs.iter().map(|tab| {
            let view = &tab.view;
            json!({"id":view.id,"host":ui::host(&view.url),"active":view.id==self.active,
                "visible":visible.contains(&view.id),"state":format!("{:?}",tab.state),
                "hidden_ms":tab.hidden_since.map(|since|since.elapsed().as_millis() as u64),
                "loading":view.loading,"audio":view.audio,"media":view.media,"pinned":view.pinned,
                "excluded":self.settings.kept_active(&view.url),"compacted":tab.compacted,
                "installable":tab.web_app.is_some(),"security":format!("{:?}",tab.connection.security(&view.url)),
                "native":tab.webview.as_ref().map(platform::memory_status)})
        }).collect();
        let webviews: Vec<_> = self.tabs.iter().filter_map(|tab|tab.webview.as_ref()).collect();
        let processes = platform::processes(&webviews);
        let total_ws: u64 = processes.iter().map(|process|process.private_ws).sum();
        let total_commit: u64 = processes.iter().map(|process|process.commit).sum();
        let processes: Vec<_> = processes.iter().map(|process|json!({"pid":process.pid,"kind":process.kind,
            "private_ws":process.private_ws,"commit":process.commit})).collect();
        json!({"pid":std::process::id(),"app_window":self.installed_app.is_some(),"chrome_height":super::chrome_height(&self.window),"mode":format!("{:?}",self.memory_mode),"compact_idle":self.compact_idle,
            "adblock":platform::adblock_memory_status(),
            "tabs":tabs,"processes":processes,"private_ws":total_ws,"commit":total_commit})
    }
}
