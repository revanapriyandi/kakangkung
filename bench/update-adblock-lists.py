#!/usr/bin/env python3
"""Refresh bundled official lists; validate without modifying files unless --write is given.

Downloads are bounded and redirects stay on the explicit source allowlist. All
files are staged before atomic per-file replacement; the manifest is installed
last. A replacement failure attempts to restore the original files. This is a
maintenance command, not an application background updater or release command.
"""

import argparse
import copy
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import tempfile
import urllib.request

MAX_LIST_BYTES = 8 * 1024 * 1024
SOURCES = {
    "easylist.txt": "https://easylist.to/easylist/easylist.txt",
    "easyprivacy.txt": "https://easylist.to/easylist/easyprivacy.txt",
}
DEFAULT_MANIFEST = Path(__file__).resolve().parents[1] / "assets/filterlists/manifest.json"


def validate_source(url):
    if url not in SOURCES.values():
        raise ValueError(f"Source must be an allowlisted official HTTPS URL: {url}")
    return url


class OfficialRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        validate_source(new_url)
        return super().redirect_request(request, response, code, message, headers, new_url)


def validate_list(data):
    if len(data) > MAX_LIST_BYTES:
        raise ValueError("Filter list exceeds 8 MiB")
    try:
        text = data.decode("utf-8-sig", errors="strict")
    except UnicodeDecodeError as error:
        raise ValueError("Filter list is not valid UTF-8") from error
    first_line = text.split("\n", 1)[0].rstrip("\r")
    if not re.fullmatch(r"\[Adblock Plus(?: \d+(?:\.\d+)*)?\]", first_line):
        raise ValueError("Filter list has no valid Adblock Plus header")
    return data


def fetch_list(source):
    validate_source(source)
    opener = urllib.request.build_opener(OfficialRedirects())
    request = urllib.request.Request(source, headers={
        "User-Agent": "Kakangkung-Filter-Maintenance/1.0", "Accept": "text/plain",
    })
    with opener.open(request, timeout=30) as response:
        validate_source(response.geturl())
        length = response.headers.get("Content-Length")
        if length is not None and int(length) > MAX_LIST_BYTES:
            raise ValueError("Filter list exceeds 8 MiB")
        data = response.read(MAX_LIST_BYTES + 1)
    return validate_list(data)


def atomic_install(directory, replacements):
    """Stage all new bytes and rollback copies before replacing any tracked file."""
    with tempfile.TemporaryDirectory(prefix=".filter-update-", dir=directory) as temporary:
        stage = Path(temporary)
        originals = {}
        for index, (name, data) in enumerate(replacements.items()):
            target = directory / name
            if target.is_symlink():
                raise ValueError(f"Refusing to replace a symlink: {target}")
            original = target.read_bytes() if target.exists() else None
            originals[name] = original
            if original is not None:
                (stage / f"old-{index}").write_bytes(original)
            with (stage / f"new-{index}").open("wb") as output:
                output.write(data)
                output.flush()
                os.fsync(output.fileno())
        installed = []
        try:
            for index, name in enumerate(replacements):
                os.replace(stage / f"new-{index}", directory / name)
                installed.append((index, name))
        except OSError as error:
            failures = []
            for index, name in reversed(installed):
                try:
                    if originals[name] is None:
                        (directory / name).unlink()
                    else:
                        os.replace(stage / f"old-{index}", directory / name)
                except OSError as restore_error:
                    recovery = ""
                    if originals[name] is not None:
                        try:
                            with tempfile.NamedTemporaryFile(prefix=f".filter-recovery-{name}-", suffix=".bak",
                                                             dir=directory, delete=False) as backup:
                                backup.write(originals[name])
                                recovery = f"; original retained at {backup.name}"
                        except OSError as backup_error:
                            recovery = f"; could not retain recovery copy: {backup_error}"
                    failures.append(f"{name}: {restore_error}{recovery}")
            if failures:
                raise RuntimeError("Update failed and rollback was incomplete: " + "; ".join(failures)) from error
            raise


def update_bundle(manifest_path, write=False, fetcher=None):
    manifest_path = Path(manifest_path).resolve()
    if manifest_path.name != "manifest.json":
        raise ValueError("Manifest filename must be manifest.json")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    entries = manifest.get("files")
    if not isinstance(entries, list) or not all(isinstance(entry, dict) for entry in entries):
        raise ValueError("Manifest must contain a files array")
    for name, source in SOURCES.items():
        matching = [entry for entry in entries if entry.get("file") == name]
        if len(matching) != 1 or matching[0].get("source") != source:
            raise ValueError(f"Manifest must list {name} exactly once with its official HTTPS source")

    fetcher = fetch_list if fetcher is None else fetcher
    replacements = {}
    updated = copy.deepcopy(manifest)
    report = []
    for entry in updated["files"]:
        name = entry.get("file")
        if name not in SOURCES:
            continue  # License records and unrelated manifest metadata remain intact.
        data = validate_list(fetcher(validate_source(entry["source"])))
        digest = hashlib.sha256(data).hexdigest()
        old = entry.get("sha256", "unknown")
        entry.update(bytes=len(data), sha256=digest)
        replacements[name] = data
        report.append({"file": name, "bytes": len(data), "old_sha256": old, "sha256": digest})

    updated["retrieved_utc"] = datetime.now(timezone.utc).isoformat()
    replacements[manifest_path.name] = (json.dumps(updated, indent=2, ensure_ascii=False) + "\n").encode("utf-8")
    if write:
        atomic_install(manifest_path.parent, replacements)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--write", action="store_true", help="Replace validated snapshots and manifest (default: dry run)")
    options = parser.parse_args()
    try:
        report = update_bundle(options.manifest, options.write)
    except (OSError, ValueError, RuntimeError) as error:
        parser.exit(1, f"Update failed: {error}\n")
    print("Updated local bundle." if options.write else "Dry run: validated downloads; no files changed.")
    for entry in report:
        print(f"{entry['file']}: {entry['bytes']} bytes; SHA-256 {entry['old_sha256']} -> {entry['sha256']}")
    if options.write:
        print("Run filter engine and browser request checks before committing or releasing this bundle.")


if __name__ == "__main__":
    main()
