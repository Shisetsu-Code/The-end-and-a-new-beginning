"""Behavioral tests; all remote effects stay inside FakeGitHub."""
import hashlib
import http.client
import importlib.util
import io
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
import urllib.parse
import urllib.error
from datetime import datetime, timedelta, timezone
from email.message import Message
from email.utils import format_datetime
import subprocess
from types import SimpleNamespace
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / "upload_evidence_stream.py"


def load_uploader(case):
    if not SCRIPT.is_file():
        case.fail("The uploader must produce a reconstructable TAR without local archives")
    spec = importlib.util.spec_from_file_location("upload_evidence_stream", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class FakeGitHub:
    def __init__(self, private=True):
        self.private = private
        self.releases = []
        self.assets = []
        self.payloads = {}
        self.upload_calls = 0
        self.fail_at = None
        self.after_upload = None
        self.corrupt_digest = False
        self.sleep = lambda _: None

    def get_repo(self, repo):
        return {"private": self.private, "default_branch": "main", "size": 1}

    def list_releases(self, repo):
        return self.releases.copy()

    def create_release(self, repo, tag, body):
        release = {"id": 7, "tag_name": tag, "body": body, "draft": True,
                   "html_url": "https://github.com/owner/evidence/releases/tag/test"}
        self.releases.append(release)
        return release

    def list_assets(self, repo, release_id):
        return [a.copy() for a in self.assets]

    def upload_asset(self, repo, release_id, name, data):
        self.upload_calls += 1
        if self.upload_calls == self.fail_at:
            raise OSError("simulated interrupted connection")
        data = bytes(data)
        digest = hashlib.sha256(data).hexdigest()
        if self.corrupt_digest:
            digest = "0" * 64
        asset = {"id": len(self.assets) + 1, "name": name, "size": len(data),
                 "state": "uploaded", "digest": "sha256:" + digest}
        self.assets.append(asset)
        self.payloads[name] = data
        if self.after_upload:
            self.after_upload()
        return asset.copy()

    def delete_starter(self, repo, asset):
        self.assets.remove(asset)

    def publish_release(self, repo, release_id):
        self.releases[0]["draft"] = False
        return self.releases[0]


class UploadTests(unittest.TestCase):
    def setUp(self):
        self.uploader = load_uploader(self)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.source = Path(self.temp.name) / "copia ñ"
        self.source.mkdir()
        (self.source / "vacío").mkdir()
        (self.source / "sin datos.txt").write_bytes(b"")
        self.payload = bytes(range(256)) * 40
        (self.source / "sesión.bin").write_bytes(self.payload)
        self.client = FakeGitHub()

    def run_upload(self, **kwargs):
        return self.uploader.upload(self.source, "owner/evidence", "test", 4096,
                                    self.client, progress=lambda _: None, **kwargs)

    def test_reconstructs_names_empty_directories_and_binary_across_parts(self):
        (self.source / "program.exe").write_bytes(b"MZ fixture")
        (self.source / "run.bat").write_bytes(b"@echo off\r\n")
        result = self.run_upload()
        manifest = json.loads(self.client.payloads["manifest.json"])
        tar_bytes = b"".join(self.client.payloads[p["name"]] for p in manifest["parts"])
        self.assertGreater(len(manifest["parts"]), 1)
        self.assertEqual(hashlib.sha256(tar_bytes).hexdigest(), manifest["tar_sha256"])
        self.assertEqual(len(tar_bytes), manifest["tar_bytes"])
        self.assertTrue(all(p["size"] <= 4096 for p in manifest["parts"]))
        with tarfile.open(fileobj=io.BytesIO(tar_bytes), mode="r:") as archive:
            self.assertEqual(archive.extractfile("copia ñ/sesión.bin").read(), self.payload)
            self.assertEqual(archive.extractfile("copia ñ/sin datos.txt").read(), b"")
            self.assertEqual(archive.extractfile("copia ñ/program.exe").read(), b"MZ fixture")
            self.assertEqual(archive.extractfile("copia ñ/run.bat").read(), b"@echo off\r\n")
            self.assertTrue(archive.getmember("copia ñ/vacío").isdir())
        self.assertFalse(self.client.releases[0]["draft"])
        self.assertEqual(result, self.client.releases[0]["html_url"])

    def test_resume_skips_verified_chunks_and_reuses_completed_release(self):
        self.client.fail_at = 2
        with self.assertRaises(OSError):
            self.run_upload()
        first = self.client.assets[0].copy()
        self.assertNotIn("manifest.json", self.client.payloads)
        self.client.fail_at = None
        self.run_upload()
        self.assertEqual(sum(a["name"] == first["name"] for a in self.client.assets), 1)
        uploads = self.client.upload_calls
        self.run_upload()
        self.assertEqual(self.client.upload_calls, uploads)

    def test_changed_source_cannot_resume_existing_upload(self):
        self.client.fail_at = 2
        with self.assertRaises(OSError):
            self.run_upload()
        (self.source / "sin datos.txt").write_bytes(b"changed")
        self.client.fail_at = None
        uploads = self.client.upload_calls
        with self.assertRaises(self.uploader.UploadError):
            self.run_upload()
        self.assertEqual(self.client.upload_calls, uploads)

    def test_mutation_during_stream_leaves_draft_without_manifest(self):
        def mutate_once():
            self.client.after_upload = None
            (self.source / "sin datos.txt").write_bytes(b"changed")
        self.client.after_upload = mutate_once
        with self.assertRaises(self.uploader.UploadError):
            self.run_upload()
        self.assertNotIn("manifest.json", self.client.payloads)
        self.assertTrue(self.client.releases[0]["draft"])

    def test_mutation_during_last_partial_upload_prevents_completion(self):
        def mutate_last():
            if len(self.client.assets) == 4:
                self.client.after_upload = None
                (self.source / "sin datos.txt").write_bytes(b"changed at final upload")
        self.client.after_upload = mutate_last
        with self.assertRaises(self.uploader.UploadError):
            self.uploader.upload(self.source, "owner/evidence", "test", 6000,
                                 self.client, progress=lambda _: None)
        self.assertNotIn("manifest.json", self.client.payloads)
        self.assertTrue(self.client.releases[0]["draft"])

    def test_lost_post_response_reconciles_without_duplicate_part(self):
        real_upload = self.client.upload_asset
        lost = []
        def lost_response(*args):
            asset = real_upload(*args)
            if not lost:
                lost.append(asset["name"])
                raise self.uploader.APIError("network")
            return asset
        self.client.upload_asset = lost_response
        self.run_upload()
        self.assertEqual(sum(a["name"] == lost[0] for a in self.client.assets), 1)
        self.assertIn("manifest.json", self.client.payloads)
        self.assertFalse(self.client.releases[0]["draft"])

    def test_public_repository_is_rejected_before_release_or_upload(self):
        self.client.private = False
        with self.assertRaises(self.uploader.UploadError):
            self.run_upload()
        self.assertEqual(self.client.releases, [])
        self.assertEqual(self.client.assets, [])

    def test_remote_digest_mismatch_does_not_mark_complete(self):
        self.client.corrupt_digest = True
        with self.assertRaises(self.uploader.UploadError):
            self.run_upload()
        self.assertNotIn("manifest.json", self.client.payloads)
        self.assertTrue(self.client.releases[0]["draft"])

    def test_foreign_asset_is_not_overwritten_or_deleted(self):
        self.client.fail_at = 2
        with self.assertRaises(OSError):
            self.run_upload()
        self.client.assets.append({"id": 99, "name": "other.zip", "size": 10,
                                   "state": "uploaded", "digest": "sha256:" + "0" * 64})
        before = self.client.assets.copy()
        with self.assertRaises(self.uploader.UploadError):
            self.run_upload()
        self.assertEqual(self.client.assets, before)

    def test_symlink_is_preserved_without_reading_target(self):
        target = Path(self.temp.name) / "outside-secret.txt"
        target.write_bytes(b"must not be uploaded")
        try:
            (self.source / "link").symlink_to(target)
        except (OSError, NotImplementedError):
            self.skipTest("symlink creation is unavailable")
        original_link = os.readlink(self.source / "link")
        self.run_upload()
        manifest = json.loads(self.client.payloads["manifest.json"])
        tar_bytes = b"".join(self.client.payloads[p["name"]] for p in manifest["parts"])
        with tarfile.open(fileobj=io.BytesIO(tar_bytes), mode="r:") as archive:
            member = archive.getmember("copia ñ/link")
            self.assertTrue(member.issym())
            self.assertEqual(member.linkname, original_link)
        self.assertNotIn(b"must not be uploaded", tar_bytes)

    def test_windows_path_and_handle_metadata_can_differ_for_the_same_file(self):
        executable = self.source / "program.exe"
        executable.write_bytes(b"binary fixture")
        executable.chmod(0o777)
        real_fstat = os.fstat
        def windows_handle(fd):
            original = real_fstat(fd)
            fields = {name: getattr(original, name) for name in (
                "st_dev", "st_ino", "st_mode", "st_size", "st_mtime_ns", "st_ctime_ns")}
            # Windows path ctime is birth time, fd ctime is change time.
            # Path-only extension inference also adds executable mode bits.
            fields["st_ctime_ns"] += 1_000_000
            fields["st_mode"] &= ~0o111
            return SimpleNamespace(**fields)
        with mock.patch.object(self.uploader, "WINDOWS", True, create=True), \
             mock.patch.object(self.uploader.os, "fstat", side_effect=windows_handle):
            self.run_upload()
        manifest = json.loads(self.client.payloads["manifest.json"])
        tar_bytes = b"".join(self.client.payloads[p["name"]] for p in manifest["parts"])
        with tarfile.open(fileobj=io.BytesIO(tar_bytes), mode="r:") as archive:
            self.assertEqual(archive.extractfile("copia ñ/program.exe").read(), b"binary fixture")

    def test_windows_handle_ctime_change_during_read_is_still_rejected(self):
        real_fstat = os.fstat
        calls = []
        def changing_handle(fd):
            original = real_fstat(fd)
            fields = {name: getattr(original, name) for name in (
                "st_dev", "st_ino", "st_mode", "st_size", "st_mtime_ns", "st_ctime_ns")}
            calls.append(fd)
            fields["st_ctime_ns"] += len(calls) * 1_000_000
            return SimpleNamespace(**fields)
        with mock.patch.object(self.uploader, "WINDOWS", True, create=True), \
             mock.patch.object(self.uploader.os, "fstat", side_effect=changing_handle):
            with self.assertRaisesRegex(self.uploader.UploadError, "while reading"):
                self.run_upload()
        self.assertNotIn("manifest.json", self.client.payloads)

    def test_windows_opened_replacement_inode_is_rejected(self):
        real_fstat = os.fstat
        def replaced_handle(fd):
            original = real_fstat(fd)
            fields = {name: getattr(original, name) for name in (
                "st_dev", "st_ino", "st_mode", "st_size", "st_mtime_ns", "st_ctime_ns")}
            fields["st_ino"] += 1
            return SimpleNamespace(**fields)
        with mock.patch.object(self.uploader, "WINDOWS", True, create=True), \
             mock.patch.object(self.uploader.os, "fstat", side_effect=replaced_handle):
            with self.assertRaisesRegex(self.uploader.UploadError, "before reading"):
                self.run_upload()
        self.assertNotIn("manifest.json", self.client.payloads)

    @unittest.skipUnless(os.name == "nt", "Windows junction semantics")
    def test_windows_junction_is_preserved_without_traversal(self):
        target = Path(self.temp.name) / "junction-target"
        target.mkdir()
        (target / "outside.txt").write_bytes(b"outside junction contents")
        link = self.source / "junction"
        result = subprocess.run(["cmd", "/c", "mklink", "/J", str(link), str(target)],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.run_upload()
        manifest = json.loads(self.client.payloads["manifest.json"])
        tar_bytes = b"".join(self.client.payloads[p["name"]] for p in manifest["parts"])
        with tarfile.open(fileobj=io.BytesIO(tar_bytes), mode="r:") as archive:
            member = archive.getmember("copia ñ/junction")
            self.assertTrue(member.issym())
            self.assertEqual(member.pax_headers["evidence.windows_reparse_tag"], str(0xA0000003))
        self.assertEqual(manifest["source"]["links"], 1)
        self.assertNotIn(b"outside junction contents", tar_bytes)

    def test_upload_creates_no_local_files_or_temporary_archives(self):
        before = sorted(str(p.relative_to(self.source)) for p in self.source.rglob("*"))
        with mock.patch("tempfile.TemporaryFile", side_effect=AssertionError("temp archive")), \
             mock.patch("tempfile.NamedTemporaryFile", side_effect=AssertionError("temp archive")):
            self.run_upload()
        after = sorted(str(p.relative_to(self.source)) for p in self.source.rglob("*"))
        self.assertEqual(before, after)

    def test_scan_tar_size_matches_the_real_stream(self):
        scan = self.uploader.scan_source(self.source)
        sink = io.BytesIO()
        self.uploader.stream_tar(self.source, sink, scan)
        self.assertEqual(scan["tar_bytes"], len(sink.getvalue()))
        self.assertEqual(scan["files"], 2)
        self.assertEqual(scan["directories"], 2)
        self.assertEqual(scan["source_bytes"], len(self.payload))


class RequestTests(unittest.TestCase):
    def setUp(self):
        self.uploader = load_uploader(self)

    def test_list_assets_reads_beyond_first_page(self):
        requests = []
        def opener(request, timeout):
            requests.append(request)
            query = urllib.parse.parse_qs(urllib.parse.urlsplit(request.full_url).query)
            page = [{"id": i} for i in range(100)] if query["page"] == ["1"] else [{"id": 100}]
            return io.BytesIO(json.dumps(page).encode())
        client = self.uploader.GitHubClient("not-a-real-token", opener=opener)
        self.assertEqual(len(client.list_assets("owner/evidence", 7)), 101)
        self.assertEqual(len(requests), 2)

    def test_binary_upload_has_exact_content_length_and_restricted_host(self):
        captured = []
        def opener(request, timeout):
            captured.append(request)
            return io.BytesIO(b'{"id":1}')
        client = self.uploader.GitHubClient("not-a-real-token", opener=opener)
        client.upload_asset("owner/evidence", 7, "part.tar.part", memoryview(b"abc\x00"))
        self.assertEqual(captured[0].get_header("Content-length"), "4")
        self.assertEqual(bytes(captured[0].data), b"abc\x00")
        self.assertTrue(captured[0].full_url.startswith("https://uploads.github.com/"))

    def test_partial_or_invalid_post_response_is_reconcilable(self):
        for failure in (http.client.IncompleteRead(b"partial"), b'{"id":'):
            with self.subTest(failure=type(failure).__name__):
                def opener(request, timeout):
                    if isinstance(failure, Exception):
                        raise failure
                    return io.BytesIO(failure)
                client = self.uploader.GitHubClient("not-a-real-token", opener=opener)
                with self.assertRaises(self.uploader.APIError) as error:
                    client.upload_asset("owner/evidence", 7, "part.tar.part", b"data")
                self.assertEqual(error.exception.status, "network")

    def test_retry_after_http_date_is_honored(self):
        attempts, delays = [], []
        headers = Message()
        headers["Retry-After"] = format_datetime(datetime.now(timezone.utc) + timedelta(seconds=60), usegmt=True)
        def opener(request, timeout):
            attempts.append(request)
            if len(attempts) == 1:
                raise urllib.error.HTTPError(request.full_url, 429, "rate limited", headers, None)
            return io.BytesIO(b'{"private":true}')
        client = self.uploader.GitHubClient("not-a-real-token", opener=opener, sleep=delays.append)
        self.assertTrue(client.get_repo("owner/evidence")["private"])
        self.assertEqual(len(delays), 1)
        self.assertGreater(delays[0], 30)
        self.assertLessEqual(delays[0], 60)


if __name__ == "__main__":
    unittest.main()
