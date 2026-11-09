//! In-memory previews and renderer attribution. Neither operation resumes a hidden tab.

use std::collections::HashSet;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{CapturePreviewCompletedHandler, GetProcessExtendedInfosCompletedHandler, CallDevToolsProtocolMethodCompletedHandler};
use windows::core::{Interface, BOOL, HSTRING};
use windows::Win32::Foundation::{E_ABORT, E_FAIL, HGLOBAL};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, IStream, CLSCTX_INPROC_SERVER, STATSTG, STATFLAG_NONAME, STREAM_SEEK_SET};
use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use wry::{WebView, WebViewExtWindows};

#[derive(Debug, Clone)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    /// Premultiplied BGRA, tightly packed. At most 320 × 180 × 4 bytes.
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub struct TabMemory {
    /// Entire private working sets of the renderer processes associated with this tab.
    pub private_ws: u64,
    /// These process totals can include other tabs or frames; they are not a page heap size.
    pub shared: bool,
    pub heap_only: bool,
}

fn fitted_size(width: u32, height: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 { return None; }
    let factor = (320.0 / width as f64).min(180.0 / height as f64).min(1.0);
    Some(((width as f64 * factor).floor().max(1.0) as u32,
          (height as f64 * factor).floor().max(1.0) as u32))
}

/// Call only after content has loaded, while visible, before hiding the view. Scheduling
/// errors are returned directly; asynchronous capture/decoding errors reach `done`.
pub fn capture_thumbnail(wv: &WebView, done: impl FnOnce(Result<Thumbnail, String>) + 'static) -> crate::Result<()> {
    let mut visible = BOOL(0);
    unsafe { wv.controller().IsVisible(&mut visible)?; }
    if !visible.as_bool() || crate::webview::is_suspended(wv)? {
        return Err(windows::core::Error::from(E_ABORT));
    }
    // The stream owns its HGLOBAL and releases it when the completion handler is dropped.
    let stream = unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true)? };
    let captured = stream.clone();
    let handler = CapturePreviewCompletedHandler::create(Box::new(move |hr| {
        let result = hr.and_then(|_| decode_thumbnail(&captured));
        done(result.map_err(|e| e.to_string()));
        Ok(())
    }));
    unsafe { wv.webview().CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_JPEG, &stream, &handler) }
}

fn decode_thumbnail(stream: &IStream) -> windows::core::Result<Thumbnail> {
    unsafe {
        let mut stat = STATSTG::default();
        stream.Stat(&mut stat, STATFLAG_NONAME)?;
        if stat.cbSize == 0 || stat.cbSize > 8 * 1024 * 1024 {
            return Err(windows::core::Error::from(E_FAIL));
        }
        stream.Seek(0, STREAM_SEEK_SET, None)?;
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let decoder = factory.CreateDecoderFromStream(stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
        let frame = decoder.GetFrame(0)?;
        let (mut width, mut height) = (0, 0);
        frame.GetSize(&mut width, &mut height)?;
        let (width, height) = fitted_size(width, height).ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        let scaler = factory.CreateBitmapScaler()?;
        scaler.Initialize(&frame, width, height, WICBitmapInterpolationModeFant)?;
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(&scaler, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let mut pixels = vec![0; (width * height * 4) as usize];
        converter.CopyPixels(std::ptr::null(), width * 4, &mut pixels)?;
        Ok(Thumbnail { width, height, pixels })
    }
}

/// Attribute renderers through WebView2 frame identities, never by title or URL. This is
/// a process-level snapshot, including child-frame renderers, with conservative sharing.
pub fn tab_memory(wv: &WebView, done: impl FnOnce(Result<TabMemory, String>) + 'static) -> crate::Result<()> {
    // Runtime 156 has a reported frame-iterator crash (#5793); local 157 hover tests also faulted.
    // Keep newer, unverified runtimes on the safe heap-only path until a fix is verified.
    let version = crate::identity::runtime_version(wv).unwrap_or_default();
    if !safe_frame_attribution(&version) { return heap_memory(wv,done); }
    let core: ICoreWebView2_20 = wv.webview().cast()?;
    let env: ICoreWebView2Environment13 = wv.environment().cast()?;
    let mut root = 0;
    unsafe { core.FrameId(&mut root)?; }
    if root == 0 { return Err(windows::core::Error::from(E_ABORT)); }
    let handler = GetProcessExtendedInfosCompletedHandler::create(Box::new(move |hr, infos| {
        let result = hr.and_then(|_| {
            let infos = infos.ok_or_else(|| windows::core::Error::from(E_FAIL))?;
            unsafe { attributed_memory(&infos, root) }
        });
        done(result.map_err(|e| e.to_string()));
        Ok(())
    }));
    unsafe { env.GetProcessExtendedInfos(&handler) }
}

// Parent chains are capped and cycle checked because the asynchronous list is a snapshot.
unsafe fn belongs_to(frame: &ICoreWebView2FrameInfo, root: u32) -> windows::core::Result<bool> {
    let mut frame = frame.clone();
    let mut seen = HashSet::new();
    for _ in 0..128 {
        let info: ICoreWebView2FrameInfo2 = frame.cast()?;
        let mut id = 0;
        unsafe { info.FrameId(&mut id)?; }
        if id == root { return Ok(true); }
        if id == 0 || !seen.insert(id) { return Ok(false); }
        match unsafe { info.ParentFrameInfo() } {
            Ok(parent) => frame = parent,
            Err(_) => return Ok(false), // A main frame has a null parent.
        }
    }
    Ok(false)
}

fn associated_pids(records: impl IntoIterator<Item = (u32, bool)>) -> HashSet<u32> {
    records.into_iter().filter_map(|(pid, belongs)| (belongs && pid != 0).then_some(pid)).collect()
}

unsafe fn attributed_memory(infos: &ICoreWebView2ProcessExtendedInfoCollection, root: u32) -> windows::core::Result<TabMemory> {
    let mut count = 0;
    unsafe { infos.Count(&mut count)?; }
    let mut records = Vec::new();
    for i in 0..count {
        let process = unsafe { infos.GetValueAtIndex(i)? };
        let info = unsafe { process.ProcessInfo()? };
        let mut kind = COREWEBVIEW2_PROCESS_KIND::default();
        unsafe { info.Kind(&mut kind)?; }
        if kind != COREWEBVIEW2_PROCESS_KIND_RENDERER { continue; }
        let mut pid = 0;
        unsafe { info.ProcessId(&mut pid)?; }
        if pid <= 0 { continue; }
        let frames = unsafe { process.AssociatedFrameInfos()? };
        let iter = unsafe { frames.GetIterator()? };
        let mut current = BOOL(0);
        unsafe { iter.HasCurrent(&mut current)?; }
        let mut belongs = false;
        while current.as_bool() {
            let frame = unsafe { iter.GetCurrent()? };
            if unsafe { belongs_to(&frame, root)? } { belongs = true; }
            unsafe { iter.MoveNext(&mut current)?; }
        }
        records.push((pid as u32, belongs));
    }
    let pids = associated_pids(records);
    if pids.is_empty() { return Err(windows::core::Error::from(E_FAIL)); }
    let mut private_ws = 0u64;
    for pid in pids {
        let (ws, _, created) = crate::processes::read(pid);
        if created == 0 || ws == 0 { return Err(windows::core::Error::from(E_FAIL)); }
        private_ws = private_ws.saturating_add(ws);
    }
    Ok(TabMemory { private_ws, shared: true, heap_only: false })
}

fn safe_frame_attribution(version: &str) -> bool {
    version.split('.').next().and_then(|major|major.parse::<u32>().ok()).is_some_and(|major|(120..156).contains(&major))
}

fn heap_memory(wv: &WebView, done: impl FnOnce(Result<TabMemory, String>) + 'static) -> crate::Result<()> {
    if crate::webview::is_suspended(wv)? { return Err(windows::core::Error::from(E_ABORT)); }
    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, value| {
        let memory = result.map_err(|error|error.to_string()).and_then(|_| {
            let value: serde_json::Value = serde_json::from_str(&value).map_err(|error|error.to_string())?;
            let used = value["usedSize"].as_f64().filter(|used|used.is_finite() && *used>=0.0 && *used<u64::MAX as f64).ok_or("Heap size is unavailable")?;
            Ok(TabMemory {private_ws:used as u64,shared:false,heap_only:true})
        });
        done(memory);
        Ok(())
    }));
    unsafe { wv.webview().CallDevToolsProtocolMethod(&HSTRING::from("Runtime.getHeapUsage"),&HSTRING::from("{}"),&handler) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_broken_and_unverified_runtimes_never_enumerate_frame_objects() {
        assert!(safe_frame_attribution("155.0.1234.1"));
        for version in ["156.0.4314.58","157.0.4341.42","158.0.1.1","","unknown","119.0.1.1"] { assert!(!safe_frame_attribution(version)); }
    }
    #[test]
    fn previews_preserve_shape_without_enlarging() {
        assert_eq!(fitted_size(1920, 1080), Some((320, 180)));
        assert_eq!(fitted_size(1080, 1920), Some((101, 180)));
        assert_eq!(fitted_size(32, 16), Some((32, 16)));
        assert_eq!(fitted_size(0, 120), None);
        assert_eq!(fitted_size(u32::MAX, 1), Some((320, 1)));
    }
    #[test]
    fn renderer_pids_require_attribution_and_are_counted_once() {
        assert_eq!(associated_pids([(11, true), (11, true), (12, false), (0, true), (13, true)]), HashSet::from([11, 13]));
    }
}
