//! Private working set and commit for the app tree and its WebView2 environments. Runtime
//! processes can outlive their launching window, so environment PIDs are authoritative seeds
//! even when they are outside the current app tree. Windows Task Manager may group them separately.

use std::collections::{HashMap, HashSet};
use std::cell::Cell;

use windows::core::Interface;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot,Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2};
use windows::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use wry::{WebView, WebViewExtWindows};

thread_local! {
    static HEAP_TIMER: Cell<usize> = const { Cell::new(0) };
    static HEAP_CLEANUPS: Cell<u64> = const { Cell::new(0) };
}

pub(crate) fn cancel_heap_cleanup() {
    let timer = HEAP_TIMER.with(|timer| timer.replace(0));
    if timer != 0 { unsafe { let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(None, timer); } }
}

pub(crate) fn defer_heap_cleanup() {
    cancel_heap_cleanup();
    let timer = unsafe { windows::Win32::UI::WindowsAndMessaging::SetTimer(None, 0, 3_000, Some(cleanup_idle_heap)) };
    HEAP_TIMER.with(|slot| slot.set(timer));
}

unsafe extern "system" fn cleanup_idle_heap(_: windows::Win32::Foundation::HWND, _: u32, timer: usize, _: u32) {
    if HEAP_TIMER.with(Cell::get) != timer { return; }
    cancel_heap_cleanup();
    match release_free_heap_memory() {
        Ok(()) => HEAP_CLEANUPS.with(|count| count.set(count.get().saturating_add(1))),
        Err(error) => eprintln!("idle heap cleanup: {error}"),
    }
}

pub(crate) fn heap_cleanup_count() -> u64 { HEAP_CLEANUPS.with(Cell::get) }

/// Return unused host heap pages to Windows; live allocations and child processes are untouched.
fn release_free_heap_memory() -> windows::core::Result<()> {
    use windows::Win32::System::Memory::{HeapOptimizeResources, HeapSetInformation};
    // HEAP_OPTIMIZE_RESOURCES_INFORMATION, version 1: two DWORD fields, no optional flags.
    #[repr(C)]
    struct Options { version: u32, flags: u32 }
    let options = Options { version: 1, flags: 0 };
    unsafe { HeapSetInformation(None, HeapOptimizeResources, Some((&options as *const Options).cast()), std::mem::size_of::<Options>()) }
}

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
pub(crate) fn read(pid: u32) -> (u64, u64, u64) {
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

/// Merge the app ancestry and runtime-reported seeds, following only verified child lifetimes.
/// Unknown creation times never authorize ancestry; a reused parent PID must not capture an
/// unrelated older process. Each PID appears once even when environments share a browser.
fn select_processes(procs: &HashMap<u32, (u32, String)>, me: u32, engine: impl IntoIterator<Item = u32>, mut born: impl FnMut(u32) -> u64) -> Vec<u32> {
    let exe = procs.get(&me).map(|(_, n)| n.clone()).unwrap_or_default();
    let mut root = me;
    let mut ancestors = HashSet::from([me]);
    while let Some((parent, _)) = procs.get(&root) {
        match procs.get(parent) {
            Some((_, name)) if !exe.is_empty() && *name == exe && !ancestors.contains(parent) && older_or_same(born(*parent), born(root)) => {
                ancestors.insert(*parent);
                root = *parent;
            }
            _ => break,
        }
    }
    let mut out = vec![root];
    let mut seen = HashSet::from([root]);
    for pid in engine {
        if pid != 0 && procs.contains_key(&pid) && born(pid) != 0 && seen.insert(pid) {
            out.push(pid);
        }
    }
    let mut i = 0;
    while i < out.len() {
        let parent = out[i];
        let kids: Vec<u32> = procs.iter().filter(|(pid, (pp, _))| *pp == parent && **pid != parent).map(|(pid, _)| *pid).collect();
        for kid in kids {
            if !seen.contains(&kid) && older_or_same(born(parent), born(kid)) {
                seen.insert(kid);
                out.push(kid);
            }
        }
        i += 1;
    }
    out
}

fn older_or_same(parent: u64, child: u64) -> bool {
    parent != 0 && child != 0 && parent <= child
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
            if pid > 0 { out.insert(pid as u32, kind); }
        }
    }
    out
}

/// The app tree plus every process reported by the supplied WebView2 environments, deduplicated.
/// A shared runtime belongs to the entire environment; this is not exact per-tab attribution.
pub fn processes(webviews: &[&WebView]) -> Vec<ProcessMemory> {
    let procs = snapshot();
    let mut kinds = HashMap::new();
    let mut environments = HashSet::new();
    for wv in webviews {
        let identity = wv.environment().cast::<windows::core::IUnknown>().ok().map(|e| e.as_raw() as usize);
        if identity.is_some_and(|id| !environments.insert(id)) { continue; }
        kinds.extend(webview_kinds(wv));
    }
    let mut counters = HashMap::new();
    select_processes(&procs, std::process::id(), kinds.keys().copied(), |pid| counters.entry(pid).or_insert_with(|| read(pid)).2)
        .into_iter()
        .filter_map(|pid| {
            let (private_ws, commit, created) = read(pid);
            if counters.get(&pid).is_some_and(|(_, _, before)| *before != created) { return None; }
            let name = procs.get(&pid).map_or("", |(_, n)| n.as_str());
            let kind = kinds.get(&pid).copied().unwrap_or(if name == "kakangkung.exe" { "Kakangkung" } else { "Helper" });
            Some(ProcessMemory { pid, kind, private_ws, commit })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optimizing_unused_heap_pages_preserves_live_allocations() {
        let live = vec![0x5au8; 1_048_576];
        release_free_heap_memory().expect("Windows heap optimization");
        assert!(live.iter().all(|byte| *byte == 0x5a));
    }

    #[test]
    fn this_process_is_in_its_own_tree() {
        let procs = snapshot();
        let mut created = HashMap::new();
        let t = select_processes(&procs, std::process::id(), [], |pid| *created.entry(pid).or_insert_with(|| read(pid).2));
        assert!(t.contains(&std::process::id()));
        assert!(read(std::process::id()).0 > 0, "private working set of a running process");
    }

    #[test]
    fn merges_engine_siblings_and_descendants_without_other_webviews_or_duplicates() {
        let procs = HashMap::from([
            (10, (1, "kakangkung.exe".into())), (11, (10, "kakangkung.exe".into())),
            (20, (10, "msedgewebview2.exe".into())), (21, (20, "msedgewebview2.exe".into())),
            (30, (999, "msedgewebview2.exe".into())), (31, (30, "msedgewebview2.exe".into())),
            (40, (1, "msedgewebview2.exe".into())), (41, (40, "msedgewebview2.exe".into())),
        ]);
        let mut selected = select_processes(&procs, 11, [20, 21, 30, 30, 0], u64::from);
        selected.sort();
        assert_eq!(selected, [10, 11, 20, 21, 30, 31]);
    }

    #[test]
    fn recycled_or_unreadable_parent_does_not_capture_an_unrelated_process() {
        let procs = HashMap::from([
            (10, (1, "kakangkung.exe".into())), (20, (10, "msedgewebview2.exe".into())),
            (30, (10, "msedgewebview2.exe".into())),
        ]);
        let times = HashMap::from([(10, 100), (20, 50), (30, 0)]);
        assert_eq!(select_processes(&procs, 10, [], |pid| times[&pid]), [10]);
        assert!(!older_or_same(0, 10));
        assert!(!older_or_same(10, 0));
    }
}
