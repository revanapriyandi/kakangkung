//! Site icons for the tab strip. WebView2 hands over a page's favicon as PNG from its own cache;
//! it is decoded once per icon address with WIC to [`SIZE`]² premultiplied BGRA and kept here,
//! on the UI thread, for the frame to draw.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use webview2_com::GetFaviconCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2_15, COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapDitherTypeNone,
    WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::core::Interface;
use windows::Win32::System::Com::{CoCreateInstance, IStream, CLSCTX_INPROC_SERVER};

/// Icon edge in pixels: sharp at 16 DIPs up to 200% scale.
pub(crate) const SIZE: u32 = 32;
/// Icons kept at most; past it the cache starts over.
// ponytail: whole-cache reset, an LRU if people keep hundreds of sites open.
const MAX: usize = 256;

thread_local! {
    static ICONS: RefCell<HashMap<String, Rc<Vec<u8>>>> = RefCell::new(HashMap::new());
}

/// Is an icon loaded under `key`?
pub fn loaded(key: &str) -> bool {
    get(key).is_some()
}

/// The decoded icon at `uri` (`SIZE`² BGRA, premultiplied), if loaded.
pub(crate) fn get(uri: &str) -> Option<Rc<Vec<u8>>> {
    ICONS.with(|m| m.borrow().get(uri).cloned())
}

/// Make the page's current icon (at `uri`) available to [`get`], then call `done(uri)`. Known
/// icons and pages without one call `done` at once; a failed decode leaves the globe.
pub(crate) fn load(core: &ICoreWebView2_15, uri: String, done: Rc<dyn Fn(String)>) {
    if uri.is_empty() || get(&uri).is_some() {
        return done(uri);
    }
    let key = uri.clone();
    let handler = GetFaviconCompletedHandler::create(Box::new(move |hr, stream| {
        if let (Ok(()), Some(stream)) = (hr, stream) {
            match decode(&stream) {
                Ok(px) => ICONS.with(|m| {
                    let mut m = m.borrow_mut();
                    if m.len() >= MAX {
                        m.clear();
                    }
                    m.insert(key.clone(), Rc::new(px));
                }),
                Err(e) => eprintln!("favicon: {e}"),
            }
        }
        done(key);
        Ok(())
    }));
    // SAFETY: plain COM call; the handler is a valid COM object.
    if let Err(e) = unsafe { core.GetFavicon(COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, &handler) } {
        eprintln!("favicon: {e}");
    }
}

/// Make the image file at `path` (an extension's toolbar icon) available to [`get`] under the
/// key [`file_key`]`(path)`. Returns that key, or `None` when the file cannot be decoded.
pub fn load_file(path: &std::path::Path) -> Option<String> {
    let key = file_key(path);
    (get(&key).is_some() || load_file_as(path, &key)).then_some(key)
}

/// Make the image file at `path` (PNG, ICO…) available to [`get`] under `key`; an icon file's
/// largest picture is used. False when the file cannot be decoded.
pub fn load_file_as(path: &std::path::Path, key: &str) -> bool {
    // SAFETY: plain WIC calls; the path is a valid, NUL-terminated wide string for its duration.
    let frame = unsafe {
        let Ok(wic) = CoCreateInstance::<_, IWICImagingFactory>(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) else { return false };
        let Ok(decoder) = wic.CreateDecoderFromFilename(&windows::core::HSTRING::from(path.as_os_str()), None, windows::Win32::Foundation::GENERIC_READ, WICDecodeMetadataCacheOnDemand) else { return false };
        let count = decoder.GetFrameCount().unwrap_or(1);
        let size = |i: u32| decoder.GetFrame(i).ok().and_then(|f| {
            let (mut w, mut h) = (0u32, 0u32);
            f.GetSize(&mut w, &mut h).ok().map(|_| w * h)
        });
        let best = (0..count).max_by_key(|&i| size(i).unwrap_or(0)).unwrap_or(0);
        decoder.GetFrame(best).ok()
    };
    let Some(px) = frame.and_then(|f| f.cast().ok()).and_then(|f| scale(&f).map_err(|e| eprintln!("icon {}: {e}", path.display())).ok()) else { return false };
    ICONS.with(|m| m.borrow_mut().insert(key.to_string(), Rc::new(px)));
    true
}

/// Cache key of an icon loaded from a file.
fn file_key(path: &std::path::Path) -> String {
    format!("file:{}", path.display())
}

fn decode(stream: &IStream) -> windows::core::Result<Vec<u8>> {
    // SAFETY: plain WIC call on a stream WebView2 handed over.
    let frame = unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        wic.CreateDecoderFromStream(stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?.GetFrame(0)?
    };
    scale(&frame.cast()?)
}

/// `SIZE`² premultiplied BGRA pixels of a decoded image.
fn scale(frame: &windows::Win32::Graphics::Imaging::IWICBitmapSource) -> windows::core::Result<Vec<u8>> {
    // SAFETY: plain WIC calls on objects created here; `px` holds exactly SIZE² 4-byte pixels.
    unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let scaler = wic.CreateBitmapScaler()?;
        scaler.Initialize(frame, SIZE, SIZE, WICBitmapInterpolationModeFant)?;
        let bgra = wic.CreateFormatConverter()?;
        bgra.Initialize(&scaler, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];
        bgra.CopyPixels(std::ptr::null(), SIZE * 4, &mut px)?;
        Ok(px)
    }
}
