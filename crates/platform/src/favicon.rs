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
use windows::Win32::System::Com::{CoCreateInstance, IStream, CLSCTX_INPROC_SERVER};

/// Icon edge in pixels: sharp at 16 DIPs up to 200% scale.
pub(crate) const SIZE: u32 = 32;
/// Icons kept at most; past it the cache starts over.
// ponytail: whole-cache reset, an LRU if people keep hundreds of sites open.
const MAX: usize = 256;

thread_local! {
    static ICONS: RefCell<HashMap<String, Rc<Vec<u8>>>> = RefCell::new(HashMap::new());
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

fn decode(stream: &IStream) -> windows::core::Result<Vec<u8>> {
    // SAFETY: plain WIC calls on objects created here; `px` holds exactly SIZE² 4-byte pixels.
    unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let frame = wic.CreateDecoderFromStream(stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?.GetFrame(0)?;
        let scaler = wic.CreateBitmapScaler()?;
        scaler.Initialize(&frame, SIZE, SIZE, WICBitmapInterpolationModeFant)?;
        let bgra = wic.CreateFormatConverter()?;
        bgra.Initialize(&scaler, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];
        bgra.CopyPixels(std::ptr::null(), SIZE * 4, &mut px)?;
        Ok(px)
    }
}
