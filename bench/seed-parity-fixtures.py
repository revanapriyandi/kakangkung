"""Create only synthetic Chrome/Edge profiles for the native self-test."""
import json
import pathlib
import sqlite3
import sys
import struct
import zlib

root = pathlib.Path(sys.argv[1])
root.mkdir(parents=True, exist_ok=True)
def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data) & 0xffffffff)
pixels = b''.join(b'\x00' + bytes((40, 130, 220, 255)) * 64 for _ in range(64))
png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 64, 64, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(pixels)) + chunk(b'IEND', b'')
(root / 'avatar.png').write_bytes(png)
for vendor, browser in [("Google", "Chrome"), ("Microsoft", "Edge")]:
    base = root / vendor / browser / "User Data"
    profile = base / "Default"
    profile.mkdir(parents=True, exist_ok=True)
    with sqlite3.connect(profile / "History") as db:
        db.execute("CREATE TABLE IF NOT EXISTS urls(url TEXT, title TEXT, visit_count INTEGER, last_visit_time INTEGER, hidden INTEGER)")
        db.execute("DELETE FROM urls")
        db.execute("INSERT INTO urls VALUES(?,?,?,?,?)", ("https://fixture.example/docs", "Synthetic fixture", 2, 13300000000000000, 0))
    (base / "Local State").write_text(json.dumps({"profile": {"last_used": "Default", "info_cache": {"Default": {"name": "Synthetic fixture"}}}}), encoding="utf-8")
    (profile / "Bookmarks").write_text(json.dumps({"roots": {"bookmark_bar": {"children": [{"type": "url", "url": "https://fixture.example/docs", "name": "Synthetic fixture"}]}}}), encoding="utf-8")
