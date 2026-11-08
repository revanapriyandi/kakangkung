//! Memory of every Kakangkung process (task manager), counted the way Windows Task Manager counts
//! the "Kakangkung" entry: the whole process tree under the first `kakangkung.exe` (every window,
//! every WebView2 process, crash handler included), each at its private working set, the value of
//! Task Manager's "Memory" column. Both read `GetProcessMemoryInfo`, so the numbers match.

use std::collections::HashMap;

use windows::core::Interface;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot,Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2};
use windows::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use wry::{WebView, WebViewExtWindows};

/// One process of the browser, for the task manager.
#[derive(Debug, Clone)]
pub struct ProcessMemory {
    pub pid: u32,
    /// "Browser", "Renderer", "GPU", "Utility", "Sandbox helper", "Plugin", "Kakangkung" or "Helper".
    pub kind: &'static str,
    /// Private working set: what Task Manager's "Memory" column shows.
    pub private_ws: u64,
    /// Private commit charge.
    pub commit: u64,
}

/// (private working set, commit, creation time) of `pid`; zeros when it cannot be opened.
fn read(pid: u32) -> (u64, u64, u64) {
    // SAFETY: the handle is closed below; the counters struct size is passed as `cb`; the
    // FILETIMEs are writable locals.
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return (0, 0, 0) };
        let mut c = PROCESS_MEMORY_COUNTERS_EX2 { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32, ..Default::default() };
        let ok = GetProcessMemoryInfo(h, (&mut c as *mut PROCESS_MEMORY_COUNTERS_EX2).cast::<PROCESS_MEMORY_COUNTERS>(), c.cb).is_ok();
        let (mut created, mut a, mut b, mut d) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        let created = if GetProcessTimes(h, &mut created, &mut a, &mut b, &mut d).is_ok() { (created.dwHighDateTime as u64) << 32 | created.dwLowDateTime as u64 } else { 0 };
        let _ = CloseHandle(h);
        if ok { (c.PrivateWorkingSetSize as u64, c.PrivateUsage as u64, created) } else { (0, 0, created) }
    }
}

/// Every running process: pid -> (parent pid, exe name lower-case).
fn snapshot() -> HashMap<u32, (u32, String)> {
    let mut out = HashMap::new();
    // SAFETY: the snapshot handle is closed below; `e.dwSize` is set as the API requires.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snap, &mut e).is_ok();
        while more {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            out.insert(e.th32ProcessID, (e.th32ParentProcessID, String::from_utf16_lossy(&e.szExeFile[..len]).to_lowercase()));
            more = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(HANDLE(snap.0));
    }
    out
}

/// Pids of the tree under the first `kakangkung.exe` above (or at) this process. A child must be
/// younger than its parent, so a recycled parent pid never pulls in a stranger.
fn tree(procs: &HashMap<u32, (u32, String)>, created: &mut HashMap<u32, u64>) -> Vec<u32> {
    let mut born = |pid: u32| *created.entry(pid).or_insert_with(|| read(pid).2);
    let me = std::process::id();
    let exe = procs.get(&me).map(|(_, n)| n.clone()).unwrap_or_default();
    let mut root = me;
    while let Some((parent, _)) = procs.get(&root) {
        match procs.get(parent) {
            Some((_, name)) if *name == exe && *parent != root && born(*parent) <= born(root) => root = *parent,
            _ => break,
        }
    }
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        let parent = out[i];
        let kids: Vec<u32> = procs.iter().filter(|(pid, (pp, _))| *pp == parent && **pid != parent).map(|(pid, _)| *pid).collect();
        for kid in kids {
            if !out.contains(&kid) && born(kid) >= born(parent) {
                out.push(kid);
            }
        }
        i += 1;
    }
    out
}

/// WebView2's own name for each of its processes, when a webview is at hand.
fn webview_kinds(wv: &WebView) -> HashMap<u32, &'static str> {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    let mut out = HashMap::new();
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
            out.insert(pid as u32, kind);
        }
    }
    out
}

/// Every process Windows Task Manager counts under "Kakangkung", with its memory.
pub fn processes(wv: Option<&WebView>) -> Vec<ProcessMemory> {
    let procs = snapshot();
    let kinds = wv.map(webview_kinds).unwrap_or_default();
    let mut created = HashMap::new();
    tree(&procs, &mut created)
        .into_iter()
        .map(|pid| {
            let (private_ws, commit, _) = read(pid);
            let name = procs.get(&pid).map_or("", |(_, n)| n.as_str());
            let kind = kinds.get(&pid).copied().unwrap_or(if name == "kakangkung.exe" { "Kakangkung" } else { "Helper" });
            ProcessMemory { pid, kind, private_ws, commit }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_in_its_own_tree() {
        let procs = snapshot();
        let mut created = HashMap::new();
        let t = tree(&procs, &mut created);
        assert!(t.contains(&std::process::id()));
        assert!(read(std::process::id()).0 > 0, "private working set of a running process");
    }
}
