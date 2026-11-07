//! Memory of every WebView2 process (task manager).

use windows::core::Interface;
use wry::{WebView, WebViewExtWindows};

/// One process of the browser, for the task manager.
#[derive(Debug, Clone)]
pub struct ProcessMemory {
    pub pid: u32,
    /// "Browser", "Renderer", "GPU", "Utility", "Sandbox helper", "Plugin" or "Kakangkung".
    pub kind: &'static str,
    /// Private working set: what Task Manager's "Memory" column shows.
    pub private_ws: u64,
    /// Private commit charge.
    pub commit: u64,
}

fn memory_of(pid: u32) -> (u64, u64) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: the handle is closed below; the counters struct size is passed as `cb`.
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return (0, 0) };
        let mut c = PROCESS_MEMORY_COUNTERS_EX2 { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32, ..Default::default() };
        let ok = GetProcessMemoryInfo(h, (&mut c as *mut PROCESS_MEMORY_COUNTERS_EX2).cast::<PROCESS_MEMORY_COUNTERS>(), c.cb).is_ok();
        let _ = CloseHandle(h);
        if ok { (c.PrivateWorkingSetSize as u64, c.PrivateUsage as u64) } else { (0, 0) }
    }
}

/// Every WebView2 process of the environment plus this app's own process.
pub fn processes(wv: &WebView) -> Vec<ProcessMemory> {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    let me = std::process::id();
    let (ws, commit) = memory_of(me);
    let mut out = vec![ProcessMemory { pid: me, kind: "Kakangkung", private_ws: ws, commit }];
    let Ok(env8) = wv.environment().cast::<ICoreWebView2Environment8>() else { return out };
    // SAFETY: out-params are valid locals; indexes are < Count.
    unsafe {
        let Ok(list) = env8.GetProcessInfos() else { return out };
        let mut n = 0u32;
        if list.Count(&mut n).is_err() {
            return out;
        }
        for i in 0..n {
            let Ok(info) = list.GetValueAtIndex(i) else { continue };
            let (mut pid, mut kind) = (0i32, COREWEBVIEW2_PROCESS_KIND::default());
            if info.ProcessId(&mut pid).is_err() || info.Kind(&mut kind).is_err() {
                continue;
            }
            let kind = match kind {
                COREWEBVIEW2_PROCESS_KIND_BROWSER => "Browser",
                COREWEBVIEW2_PROCESS_KIND_RENDERER => "Renderer",
                COREWEBVIEW2_PROCESS_KIND_GPU => "GPU",
                COREWEBVIEW2_PROCESS_KIND_UTILITY => "Utility",
                COREWEBVIEW2_PROCESS_KIND_SANDBOX_HELPER => "Sandbox helper",
                _ => "Plugin",
            };
            let (ws, commit) = memory_of(pid as u32);
            out.push(ProcessMemory { pid: pid as u32, kind, private_ws: ws, commit });
        }
    }
    out
}
