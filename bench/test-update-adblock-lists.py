import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import urllib.request

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("update_adblock_lists", Path(__file__).with_name("update-adblock-lists.py"))
updater = importlib.util.module_from_spec(spec)
spec.loader.exec_module(updater)

LIST = b"[Adblock Plus 2.0]\n! License retained\n||ads.example^\n"


class ValidationTests(unittest.TestCase):
    def test_only_exact_official_https_sources_are_accepted(self):
        for source in updater.SOURCES.values():
            self.assertEqual(updater.validate_source(source), source)
        for source in [
            "http://easylist.to/easylist/easylist.txt", "https://evil.example/easylist.txt",
            "https://easylist.to.evil.example/easylist/easylist.txt",
            "https://easylist.to@evil.example/easylist/easylist.txt",
            "https://easylist.to/easylist/easylist.txt?redirect=evil", "file:///easylist.txt",
        ]:
            with self.subTest(source=source), self.assertRaises(ValueError):
                updater.validate_source(source)

    def test_redirects_are_checked_before_following(self):
        request = urllib.request.Request(updater.SOURCES["easylist.txt"])
        handler = updater.OfficialRedirects()
        for destination in ["http://easylist.to/easylist/easylist.txt", "https://evil.example/"]:
            with self.subTest(destination=destination), self.assertRaises(ValueError):
                handler.redirect_request(request, None, 302, "Found", {}, destination)

    def test_header_utf8_and_size_validation_preserve_original_bytes(self):
        self.assertIs(updater.validate_list(LIST), LIST)
        with_bom = b"\xef\xbb\xbf[Adblock Plus 1.1]\r\n! Copyright\r\n"
        self.assertIs(updater.validate_list(with_bom), with_bom)
        for invalid in [b"<html>Server error</html>", b"[Adblock Plus 2.0]\n\xff", b"[Adblock Plus 2.0] trailing\n"]:
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                updater.validate_list(invalid)
        with patch.object(updater, "MAX_LIST_BYTES", len(LIST) - 1), self.assertRaises(ValueError):
            updater.validate_list(LIST)

    def test_download_read_is_bounded_even_without_content_length(self):
        class Response:
            headers = {}
            requested_size = None
            def __enter__(self): return self
            def __exit__(self, *args): pass
            def geturl(self): return updater.SOURCES["easylist.txt"]
            def read(self, size):
                self.requested_size = size
                return b"x" * size
        response = Response()
        class Opener:
            def open(self, *args, **kwargs): return response
        with patch.object(updater, "MAX_LIST_BYTES", 32), patch.object(updater.urllib.request, "build_opener", return_value=Opener()):
            with self.assertRaisesRegex(ValueError, "exceeds"):
                updater.fetch_list(updater.SOURCES["easylist.txt"])
        self.assertEqual(response.requested_size, 33)


class BundleTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.manifest_path = self.directory / "manifest.json"
        self.license = {"file": "LICENSE.txt", "source": "https://license.example/terms", "sha256": "license-hash", "custom": True}
        self.manifest = {"retrieved_utc": "previous", "custom": {"preserved": True}, "files": [
            {"file": name, "source": source, "bytes": 3, "sha256": "old", "custom": "keep"}
            for name, source in updater.SOURCES.items()
        ] + [self.license]}
        self.manifest_path.write_text(json.dumps(self.manifest), encoding="utf-8")
        for name in updater.SOURCES:
            (self.directory / name).write_bytes(b"old")
        (self.directory / "LICENSE.txt").write_bytes(b"original license")

    def snapshot(self):
        return {path.name: path.read_bytes() for path in self.directory.iterdir() if path.is_file()}

    def test_default_dry_run_fetches_both_but_changes_no_files(self):
        before = self.snapshot()
        sources = []
        report = updater.update_bundle(self.manifest_path, fetcher=lambda source: sources.append(source) or LIST)
        self.assertEqual(set(sources), set(updater.SOURCES.values()))
        self.assertEqual(len(report), 2)
        self.assertEqual(self.snapshot(), before)
        self.assertEqual(len(list(self.directory.iterdir())), 4)

    def test_write_updates_hashes_preserves_license_and_uses_manifest_last(self):
        replaced = []
        replace = updater.os.replace
        def observed(source, target):
            replaced.append(Path(target).name)
            replace(source, target)
        with patch.object(updater.os, "replace", side_effect=observed):
            updater.update_bundle(self.manifest_path, write=True, fetcher=lambda _: LIST)
        updated = json.loads(self.manifest_path.read_text(encoding="utf-8"))
        self.assertEqual(replaced, ["easylist.txt", "easyprivacy.txt", "manifest.json"])
        self.assertEqual(updated["files"][-1], self.license)
        self.assertEqual(updated["custom"], self.manifest["custom"])
        self.assertNotEqual(updated["retrieved_utc"], "previous")
        self.assertEqual((self.directory / "LICENSE.txt").read_bytes(), b"original license")
        for entry in updated["files"][:2]:
            self.assertEqual(entry["bytes"], len(LIST))
            self.assertEqual(entry["sha256"], hashlib.sha256(LIST).hexdigest())
            self.assertEqual(entry["custom"], "keep")
            self.assertEqual((self.directory / entry["file"]).read_bytes(), LIST)

    def test_second_invalid_download_leaves_whole_bundle_unchanged(self):
        before = self.snapshot()
        def fetch(source):
            return LIST if source == updater.SOURCES["easylist.txt"] else b"<html>failure</html>"
        with self.assertRaises(ValueError):
            updater.update_bundle(self.manifest_path, write=True, fetcher=fetch)
        self.assertEqual(self.snapshot(), before)

    def test_replacement_failure_restores_previous_bundle(self):
        before = self.snapshot()
        replace = updater.os.replace
        calls = 0
        def failing(source, target):
            nonlocal calls
            calls += 1
            if calls == 2:
                raise PermissionError("fixture locked file")
            return replace(source, target)
        with patch.object(updater.os, "replace", side_effect=failing), self.assertRaises(PermissionError):
            updater.update_bundle(self.manifest_path, write=True, fetcher=lambda _: LIST)
        self.assertEqual(self.snapshot(), before)

    def test_manifest_source_or_duplicate_error_happens_before_download(self):
        for files in [
            [dict(self.manifest["files"][0], source="http://easylist.to/easylist/easylist.txt")] + self.manifest["files"][1:],
            self.manifest["files"] + [self.manifest["files"][0]],
        ]:
            self.manifest_path.write_text(json.dumps(dict(self.manifest, files=files)), encoding="utf-8")
            with self.subTest(files=files), self.assertRaises(ValueError):
                updater.update_bundle(self.manifest_path, write=True, fetcher=lambda _: self.fail("must not fetch"))

    def test_failed_rollback_retains_original_for_manual_recovery(self):
        replace = updater.os.replace
        calls = 0
        def failing(source, target):
            nonlocal calls
            calls += 1
            if calls in [2, 3]:
                raise PermissionError("fixture locked during update and rollback")
            return replace(source, target)
        with patch.object(updater.os, "replace", side_effect=failing), self.assertRaisesRegex(RuntimeError, "original retained at"):
            updater.update_bundle(self.manifest_path, write=True, fetcher=lambda _: LIST)
        backups = list(self.directory.glob(".filter-recovery-*.bak"))
        self.assertEqual(len(backups), 1)
        self.assertEqual(backups[0].read_bytes(), b"old")


if __name__ == "__main__":
    unittest.main()
