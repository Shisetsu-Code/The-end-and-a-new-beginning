#!/usr/bin/env python3
"""Upload an uncompressed TAR in RAM-sized parts to a private GitHub release.

Python 3.10+, standard library only. The source is read-only; no archive,
checkpoint, clone, or temporary upload file is written locally. See
docs/EVIDENCE-UPLOAD.md for PowerShell instructions and limits.
"""
import argparse
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tarfile
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from email.utils import parsedate_to_datetime


FORMAT = "evidence-tar-parts-v1"
MANIFEST = "manifest.json"
MAX_ASSETS = 1000
PART_RE = re.compile(r"part-([0-9]{5})-([0-9a-f]{64})\.tar\.part\Z")
BODY_PREFIX = "Uncompressed evidence upload. Do not edit this metadata.\n\n"


class UploadError(Exception):
    """An actionable error, safe to display without HTTP bodies or credentials."""


class APIError(UploadError):
    def __init__(self, status, retry_after=0):
        self.status = status
        self.retry_after = retry_after
        super().__init__(f"GitHub HTTP {status}; check access, connectivity and the release.")


def _json_bytes(value):
    return json.dumps(value, ensure_ascii=True, sort_keys=True,
                      separators=(",", ":")).encode("utf-8")


def _identity(st):
    return (st.st_dev, st.st_ino, st.st_mode, st.st_size,
            st.st_mtime_ns, st.st_ctime_ns)


def _tar_info(path, name, st):
    info = tarfile.TarInfo(name)
    info.mode = stat.S_IMODE(st.st_mode)
    info.mtime = st.st_mtime_ns // 1_000_000_000
    info.pax_headers["mtime"] = f"{info.mtime}.{st.st_mtime_ns % 1_000_000_000:09d}"
    reparse = getattr(st, "st_file_attributes", 0) & 0x400
    tag = getattr(st, "st_reparse_tag", 0)
    if reparse and tag not in (0xA000000C, 0xA0000003):
        raise UploadError(f"Unsupported Windows reparse point: {path}")
    if stat.S_ISLNK(st.st_mode) or reparse:
        info.type = tarfile.SYMTYPE
        info.linkname = os.readlink(path)
        if reparse:
            info.pax_headers["evidence.windows_reparse_tag"] = str(tag)
    elif stat.S_ISDIR(st.st_mode):
        info.type = tarfile.DIRTYPE
    elif stat.S_ISREG(st.st_mode):
        info.type = tarfile.REGTYPE
        info.size = st.st_size
    else:
        raise UploadError(f"Unsupported special file: {path}")
    return info


def _entries(source):
    def visit(path, name):
        st = path.lstat()
        info = _tar_info(path, name, st)
        yield path, st, info
        if info.isdir():
            # Keep only one directory's names per depth, never the whole tree.
            with os.scandir(path) as entries:
                names = sorted(entry.name for entry in entries)
            for child in names:
                yield from visit(path / child, name + "/" + child)
    yield from visit(source, source.name)


def _record(hasher, st, info):
    # ctime/inode catch replacements too; atime is deliberately not included.
    hasher.update(_json_bytes([info.name, _identity(st), info.linkname]))
    hasher.update(info.tobuf(format=tarfile.PAX_FORMAT, encoding="utf-8"))


def scan_source(source):
    """Return constant-size metadata and exact TAR size without reading file bodies."""
    source = Path(source)
    if not source.is_dir() or not source.name:
        raise UploadError("--source must be an existing named directory.")
    result = {"root_name": source.name, "files": 0, "directories": 0,
              "links": 0, "source_bytes": 0}
    fingerprint = hashlib.sha256()
    archive_bytes = 1024  # TAR end markers; tarfile pads to a 10 KiB record.
    for _, st, info in _entries(source):
        _record(fingerprint, st, info)
        archive_bytes += len(info.tobuf(format=tarfile.PAX_FORMAT, encoding="utf-8"))
        archive_bytes += (info.size + 511) // 512 * 512
        if info.isfile():
            result["files"] += 1
            result["source_bytes"] += info.size
        elif info.isdir():
            result["directories"] += 1
        else:
            result["links"] += 1
    result["tar_bytes"] = (archive_bytes + tarfile.RECORDSIZE - 1) // tarfile.RECORDSIZE * tarfile.RECORDSIZE
    result["fingerprint"] = fingerprint.hexdigest()
    return result


def stream_tar(source, sink, expected_scan):
    """Write a deterministic TAR to sink; fail if the source changes during reading."""
    fingerprint = hashlib.sha256()
    with tarfile.open(fileobj=sink, mode="w|", format=tarfile.PAX_FORMAT,
                      encoding="utf-8", dereference=False) as archive:
        for path, st, info in _entries(Path(source)):
            _record(fingerprint, st, info)
            if info.isfile():
                with path.open("rb") as body:
                    if _identity(os.fstat(body.fileno())) != _identity(st):
                        raise UploadError(f"Source changed before reading: {path}")
                    archive.addfile(info, body)
                    if _identity(os.fstat(body.fileno())) != _identity(st):
                        raise UploadError(f"Source changed while reading: {path}")
            else:
                archive.addfile(info)
            # tarfile otherwise retains every TarInfo even in streaming mode.
            archive.members.clear()
    if fingerprint.hexdigest() != expected_scan["fingerprint"]:
        raise UploadError("Source changed after the initial scan; use a stable folder and a new --tag.")
    if scan_source(Path(source)) != expected_scan:
        raise UploadError("Source changed during upload; no completion manifest was written.")


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise APIError(code)


class GitHubClient:
    """HTTPS GitHub API only; token never enters URLs, logs or child command arguments."""
    def __init__(self, token, opener=None, sleep=time.sleep):
        self.token = token
        self.opener = opener or urllib.request.build_opener(_NoRedirect()).open
        self.sleep = sleep

    def _request(self, method, path, data=None, upload=False, content_type="application/json"):
        host = "uploads.github.com" if upload else "api.github.com"
        if not path.startswith("/repos/") or "\r" in path or "\n" in path:
            raise UploadError("Invalid GitHub API path.")
        headers = {"Authorization": "Bearer " + self.token,
                   "Accept": "application/vnd.github+json",
                   "X-GitHub-Api-Version": "2022-11-28",
                   "User-Agent": FORMAT}
        if data is not None:
            headers.update({"Content-Type": content_type, "Content-Length": str(len(data))})
        request = urllib.request.Request("https://" + host + path, data=data,
                                         headers=headers, method=method)
        # POSTs are reconciled by the caller before another write is attempted.
        attempts = 4 if method == "GET" else 1
        for attempt in range(attempts):
            try:
                with self.opener(request, timeout=180) as response:
                    raw = response.read(4 * 1024 * 1024)
                return json.loads(raw) if raw else None
            except urllib.error.HTTPError as exc:
                retry = exc.headers.get("Retry-After", "0")
                try:
                    delay = int(retry) if retry.isdigit() else max(0, math.ceil(
                        (parsedate_to_datetime(retry) - datetime.now(timezone.utc)).total_seconds()))
                except (TypeError, ValueError, OverflowError):
                    delay = 0
                exc.close()
                error = APIError(exc.code, delay)
                transient = exc.code in (429, 500, 502, 503, 504) or (exc.code == 403 and delay > 0)
                if not transient or attempt + 1 == attempts:
                    raise error from None
            except (urllib.error.URLError, TimeoutError, ConnectionError, OSError,
                    http.client.HTTPException, json.JSONDecodeError, UnicodeError):
                delay = 0
                if attempt + 1 == attempts:
                    raise APIError("network") from None
            self.sleep(max(delay, 2 ** attempt))

    def _list(self, path):
        result = []
        for page in range(1, 10001):
            batch = self._request("GET", f"{path}?per_page=100&page={page}")
            if not isinstance(batch, list):
                raise UploadError("GitHub returned an invalid paginated response.")
            result.extend(batch)
            if len(batch) < 100:
                return result
        raise UploadError("GitHub pagination exceeded the supported range.")

    def get_repo(self, repo):
        return self._request("GET", f"/repos/{repo}")

    def list_releases(self, repo):
        return self._list(f"/repos/{repo}/releases")

    def create_release(self, repo, tag, body):
        payload = {"tag_name": tag, "name": tag, "body": body, "draft": True,
                   "prerelease": False, "make_latest": "false"}
        return self._request("POST", f"/repos/{repo}/releases", _json_bytes(payload))

    def list_assets(self, repo, release_id):
        return self._list(f"/repos/{repo}/releases/{release_id}/assets")

    def upload_asset(self, repo, release_id, name, data):
        path = f"/repos/{repo}/releases/{release_id}/assets?" + urllib.parse.urlencode({"name": name})
        return self._request("POST", path, data, upload=True, content_type="application/octet-stream")

    def delete_starter(self, repo, asset):
        if asset.get("state") != "starter" or asset.get("size") != 0:
            raise UploadError("Refusing to delete a non-empty release asset.")
        return self._request("DELETE", f"/repos/{repo}/releases/assets/{int(asset['id'])}")

    def publish_release(self, repo, release_id):
        return self._request("PATCH", f"/repos/{repo}/releases/{release_id}",
                             _json_bytes({"draft": False, "make_latest": "false"}))


def _verified(asset, name, size, digest):
    if (asset.get("name") != name or asset.get("state") != "uploaded"
            or asset.get("size") != size or asset.get("digest") != "sha256:" + digest):
        raise UploadError(f"Remote size/state/SHA-256 verification failed: {name}. Nothing was overwritten.")


def _assets(client, repo, release_id):
    assets = client.list_assets(repo, release_id)
    by_name = {}
    positions = set()
    for asset in assets:
        name = asset.get("name", "")
        match = PART_RE.fullmatch(name)
        if name != MANIFEST and not match:
            raise UploadError("This release contains unrelated assets; use a new --tag.")
        if name in by_name or (match and match[1] in positions):
            raise UploadError("This release contains conflicting parts; use a new --tag.")
        if match:
            positions.add(match[1])
        by_name[name] = asset
    return by_name


def _put_asset(client, repo, release, assets, name, data, digest):
    for attempt in range(4):
        old = assets.get(name)
        if old and old.get("state") == "starter" and old.get("size") == 0:
            if not release.get("draft"):
                raise UploadError("An incomplete asset exists in a published release; use a new --tag.")
            client.delete_starter(repo, old)
            del assets[name]
            old = None
        if old:
            _verified(old, name, len(data), digest)
            return old, False
        if not release.get("draft"):
            raise UploadError("The published release is missing a part; use a new --tag.")
        try:
            asset = client.upload_asset(repo, release["id"], name, data)
            _verified(asset, name, len(data), digest)
            assets[name] = asset
            return asset, True
        except APIError as exc:
            if exc.status not in ("network", 403, 422, 429, 500, 502, 503, 504):
                raise
            # A lost POST response may still mean the bytes reached GitHub.
            assets.clear()
            assets.update(_assets(client, repo, release["id"]))
            if name in assets and assets[name].get("state") == "uploaded":
                _verified(assets[name], name, len(data), digest)
                return assets[name], False
            if attempt == 3 or (exc.status == 403 and exc.retry_after == 0):
                raise
            client.sleep(max(exc.retry_after, 2 ** attempt))
    raise UploadError("Upload retry limit reached; run the same command to resume.")


class _Parts:
    def __init__(self, part_bytes, client, repo, release, assets, progress):
        self.limit = part_bytes
        self.client, self.repo, self.release, self.assets = client, repo, release, assets
        self.progress = progress
        self.buffer = bytearray(part_bytes)
        self.used = 0
        self.parts = []
        self.total = 0
        self.sha = hashlib.sha256()
        self.failed = False

    def write(self, data):
        # tarfile may flush again while unwinding an earlier write exception.
        # Do not let cleanup publish duplicate/reordered bytes after failure.
        if self.failed:
            return len(data)
        try:
            return self._write(data)
        except BaseException:
            self.failed = True
            raise

    def _write(self, data):
        view = memoryview(data)
        self.sha.update(view)
        self.total += len(view)
        offset = 0
        while offset < len(view):
            amount = min(self.limit - self.used, len(view) - offset)
            self.buffer[self.used:self.used + amount] = view[offset:offset + amount]
            self.used += amount
            offset += amount
            if self.used == self.limit:
                self._send()
        return len(data)

    def _send(self):
        if not self.used:
            return
        index = len(self.parts) + 1
        if index >= MAX_ASSETS:
            raise UploadError("Too many parts for one release; increase --part-mib and use a new --tag.")
        body = memoryview(self.buffer)[:self.used]
        digest = hashlib.sha256(body).hexdigest()
        name = f"part-{index:05d}-{digest}.tar.part"
        for existing in self.assets:
            match = PART_RE.fullmatch(existing)
            if match and int(match[1]) == index and existing != name:
                raise UploadError("Source bytes differ from an uploaded part; use a new --tag.")
        asset, uploaded = _put_asset(self.client, self.repo, self.release,
                                     self.assets, name, body, digest)
        self.parts.append({"name": name, "size": self.used, "sha256": digest,
                           "asset_id": asset["id"]})
        self.progress(f"Part {index}: {'uploaded' if uploaded else 'verified, already present'} ({self.used / 1048576:.1f} MiB)")
        self.used = 0

    def finish(self):
        self._send()


def upload(source, repo, tag, part_bytes, client, progress=print):
    """Upload or resume; return the release URL only after all hashes are verified."""
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo):
        raise UploadError("--repo must be OWNER/NAME.")
    if not tag or any(ord(ch) < 32 for ch in tag):
        raise UploadError("Invalid release tag.")
    if part_bytes <= 0 or part_bytes >= 2 * 1024 ** 3:
        raise UploadError("Each part must be positive and smaller than 2 GiB.")
    source = Path(source).resolve(strict=True)
    scan = scan_source(source)
    count = math.ceil(scan["tar_bytes"] / part_bytes)
    if count + 1 > MAX_ASSETS:
        raise UploadError(f"This needs {count} parts; increase --part-mib (maximum 999 parts per release).")
    progress(f"Scan: {scan['files']} files, {scan['directories']} directories, {scan['links']} links; {scan['source_bytes'] / 1024**3:.2f} GiB; {count} parts.")
    if scan["links"]:
        progress("Links/junctions are stored as links. Their targets are not followed.")
    repository = client.get_repo(repo)
    if repository.get("private") is not True:
        raise UploadError("Evidence must go to a PRIVATE repository; create one with gh repo create --private --add-readme.")
    config = {"format": FORMAT, "source": scan, "part_bytes": part_bytes}
    body = BODY_PREFIX + _json_bytes(config).decode("ascii")
    matches = [r for r in client.list_releases(repo) if r.get("tag_name") == tag]
    if len(matches) > 1:
        raise UploadError("Multiple releases match --tag; resolve this before uploading.")
    if matches:
        release = matches[0]
        if release.get("body") != body:
            raise UploadError("Source metadata or part size changed, or tag belongs to another upload. Use a new --tag.")
    else:
        # If this POST loses its response, rerunning finds the same draft by tag.
        release = client.create_release(repo, tag, body)
    assets = _assets(client, repo, release["id"])
    if len(assets) > count + 1:
        raise UploadError("Release contains extra parts; use a new --tag.")
    sink = _Parts(part_bytes, client, repo, release, assets, progress)
    stream_tar(source, sink, scan)
    sink.finish()
    if scan_source(source) != scan:
        raise UploadError("Source changed while the final part was uploaded; no completion manifest was written.")
    if sink.total != scan["tar_bytes"]:
        raise UploadError("TAR size differs from preflight; no completion manifest was written.")
    wanted = {p["name"] for p in sink.parts} | {MANIFEST}
    if set(assets) - wanted:
        raise UploadError("Release contains unexpected parts; completion was not published.")
    manifest = {"format": FORMAT, "source": scan, "part_bytes": part_bytes,
                "tar_bytes": sink.total, "tar_sha256": sink.sha.hexdigest(),
                "parts": sink.parts}
    data = _json_bytes(manifest) + b"\n"
    _put_asset(client, repo, release, assets, MANIFEST, data, hashlib.sha256(data).hexdigest())
    if release.get("draft"):
        release = client.publish_release(repo, release["id"])
    url = release.get("html_url") or f"https://github.com/{repo}/releases/tag/{urllib.parse.quote(tag, safe='')}"
    progress("Complete: all parts and manifest verified. " + url)
    return url


def _token():
    value = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if not value:
        try:
            result = subprocess.run(["gh", "auth", "token", "--hostname", "github.com"],
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                    text=True, check=False, timeout=30)
        except (OSError, subprocess.TimeoutExpired):
            raise UploadError("Install GitHub CLI and run: gh auth login --hostname github.com --web") from None
        value = result.stdout.strip() if result.returncode == 0 else ""
    if not value or "\n" in value or "\r" in value:
        raise UploadError("No valid GitHub authentication. Run: gh auth login --hostname github.com --web")
    return value


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, help="Folder to read, without modifying it")
    parser.add_argument("--repo", required=True, help="Existing private repository OWNER/NAME")
    parser.add_argument("--tag", default="evidencia-final-copia-2026-11-05")
    parser.add_argument("--part-mib", type=int, default=128, help="RAM part size: 1..1024 MiB (default 128)")
    parser.add_argument("--check", action="store_true", help="Scan source only; no authentication or network")
    args = parser.parse_args(argv)
    try:
        if not 1 <= args.part_mib <= 1024:
            raise UploadError("--part-mib must be between 1 and 1024.")
        if args.check:
            scan = scan_source(Path(args.source).resolve(strict=True))
            scan["parts"] = math.ceil(scan["tar_bytes"] / (args.part_mib * 1048576))
            print(json.dumps(scan, indent=2, ensure_ascii=True))
            if scan["parts"] >= MAX_ASSETS:
                raise UploadError("Too many parts; increase --part-mib.")
        else:
            upload(args.source, args.repo, args.tag, args.part_mib * 1048576,
                   GitHubClient(_token()))
        return 0
    except KeyboardInterrupt:
        print("Interrupted. Run the same command with an unchanged folder to resume.", file=sys.stderr)
        return 130
    except (UploadError, OSError, tarfile.TarError, ValueError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
