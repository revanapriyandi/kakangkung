//! Puts Kakangkung's icon and version details into kakangkung.exe, so Windows (Task Manager,
//! Explorer, the taskbar before the window exists) shows the brand mark and the name
//! "Kakangkung" instead of a blank program. The icon is drawn by the same code as the window
//! icon (`ui::brand_icon_rgba`); the resource is compiled with the Windows SDK's rc.exe.

use std::path::{Path, PathBuf};

/// Icon sizes Windows picks from (small lists to the 256 px Explorer tile).
const SIZES: [u32; 7] = [16, 20, 24, 32, 48, 64, 256];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../ui/src/lib.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let ico = out.join("kakangkung.ico");
    std::fs::write(&ico, icon_file()).expect("writing the icon");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let rc = out.join("kakangkung.rc");
    std::fs::write(&rc, resource_script(&ico, &version)).expect("writing the resource script");
    let Some(compiler) = rc_exe() else {
        println!("cargo:warning=rc.exe (Windows SDK) not found: kakangkung.exe gets no icon");
        return;
    };
    let res = out.join("kakangkung.res");
    let status = std::process::Command::new(&compiler).arg("/nologo").arg("/fo").arg(&res).arg(&rc).status();
    match status {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", res.display()),
        other => println!("cargo:warning=rc.exe failed ({other:?}): kakangkung.exe gets no icon"),
    }
}

/// An .ico holding the brand mark at every size of `SIZES`, as 32-bit images with alpha.
fn icon_file() -> Vec<u8> {
    let images: Vec<Vec<u8>> = SIZES.iter().map(|&s| dib(s, &ui::brand_icon_rgba(s))).collect();
    let mut f = vec![0, 0, 1, 0];
    f.extend((SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * SIZES.len();
    for (s, img) in SIZES.iter().zip(&images) {
        let side = if *s >= 256 { 0 } else { *s as u8 }; // 0 means 256
        f.extend([side, side, 0, 0]);
        f.extend(1u16.to_le_bytes()); // colour planes
        f.extend(32u16.to_le_bytes()); // bits per pixel
        f.extend((img.len() as u32).to_le_bytes());
        f.extend((offset as u32).to_le_bytes());
        offset += img.len();
    }
    for img in images {
        f.extend(img);
    }
    f
}

/// One icon image: a bottom-up BGRA bitmap (header says twice the height: colour + mask) and an
/// empty AND mask (the alpha channel does the work).
fn dib(size: u32, rgba: &[u8]) -> Vec<u8> {
    let mut d = Vec::new();
    for v in [40u32, size, size * 2] {
        d.extend(v.to_le_bytes());
    }
    d.extend(1u16.to_le_bytes());
    d.extend(32u16.to_le_bytes());
    for _ in 0..6 {
        d.extend(0u32.to_le_bytes()); // compression, image size, resolution, colours
    }
    for y in (0..size).rev() {
        for x in 0..size {
            let p = ((y * size + x) * 4) as usize;
            d.extend([rgba[p + 2], rgba[p + 1], rgba[p], rgba[p + 3]]);
        }
    }
    let mask_row = size.div_ceil(32) * 4;
    d.extend(std::iter::repeat_n(0u8, (mask_row * size) as usize));
    d
}

fn resource_script(ico: &Path, version: &str) -> String {
    let nums: Vec<u32> = version.split('.').map(|n| n.parse().unwrap_or(0)).chain([0, 0, 0, 0]).take(4).collect();
    let comma = nums.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    let path = ico.display().to_string().replace('\\', "\\\\");
    format!(
        r#"1 ICON "{path}"
1 VERSIONINFO
FILEVERSION {comma}
PRODUCTVERSION {comma}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Kakangkung"
      VALUE "FileDescription", "Kakangkung"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "kakangkung"
      VALUE "OriginalFilename", "kakangkung.exe"
      VALUE "ProductName", "Kakangkung"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    )
}

/// The newest Windows SDK's x64 rc.exe (also when it is on PATH).
fn rc_exe() -> Option<PathBuf> {
    let kits = PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&kits)
        .ok()?
        .flatten()
        .map(|e| e.path().join("x64").join("rc.exe"))
        .filter(|p| p.is_file())
        .collect();
    versions.sort();
    versions.pop().or_else(|| std::process::Command::new("rc.exe").arg("/?").output().is_ok().then(|| PathBuf::from("rc.exe")))
}
