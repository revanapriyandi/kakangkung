//! The browser window itself: PNG capture.

use std::path::Path;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS, PW_CLIENTONLY};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, PW_RENDERFULLCONTENT,
};

/// PNG of the window's client area, including the WebView2 children
/// (`PrintWindow` with `PW_RENDERFULLCONTENT` asks DWM for composed content).
pub fn capture_window_png(hwnd: isize, path: &Path) -> std::io::Result<()> {
    let hwnd = HWND(hwnd as *mut _);
    let mut rc = RECT::default();
    // SAFETY: rc is a writable local; an invalid hwnd only makes the call fail.
    unsafe { GetClientRect(hwnd, &mut rc) }.map_err(std::io::Error::other)?;
    let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
    if w <= 0 || h <= 0 {
        return Err(std::io::Error::other("window has no client area"));
    }
    let mut bgra = vec![0u8; (w * h * 4) as usize];
    // SAFETY: standard GDI capture sequence; every object created here is released below,
    // and `bgra` is exactly w*h*4 bytes, matching the 32-bpp top-down BITMAPINFO.
    let lines = unsafe {
        let screen = GetDC(Some(hwnd));
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let _ = PrintWindow(hwnd, mem, PRINT_WINDOW_FLAGS(PW_CLIENTONLY.0 | PW_RENDERFULLCONTENT));
        SelectObject(mem, old);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let n = GetDIBits(mem, bmp, 0, h as u32, Some(bgra.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(Some(hwnd), screen);
        n
    };
    if lines != h {
        return Err(std::io::Error::other(format!("GetDIBits copied {lines} of {h} lines")));
    }
    let rgb: Vec<u8> = bgra.as_chunks::<4>().0.iter().flat_map(|p| [p[2], p[1], p[0]]).collect();
    std::fs::write(path, encode_png(w as u32, h as u32, &rgb))
}

/// Minimal PNG writer: 8-bit RGB, zlib with stored (uncompressed) blocks.
// Simplification: stored deflate makes ~3 MB files for a 1280x800 window; fine for test screenshots.
pub(crate) fn encode_png(w: u32, h: u32, rgb: &[u8]) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    let mut raw = Vec::with_capacity(rgb.len() + h as usize);
    for row in rgb.chunks_exact(w as usize * 3) {
        raw.push(0); // filter: none
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push((i + 1 == blocks.len()) as u8);
        z.extend_from_slice(&(b.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(b.len() as u16)).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn png_has_valid_structure() {
        let png = super::encode_png(2, 1, &[255, 0, 0, 0, 0, 255]);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        // Well-known IEND chunk with its fixed CRC.
        assert_eq!(&png[png.len() - 12..], b"\0\0\0\0IEND\xae\x42\x60\x82");
    }
}
