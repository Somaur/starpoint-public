"""Rebuild the service/client prefix and reuse the pinned upstream CDN payload.

This is a distribution build, not a rebuild of the proprietary game or the
upstream Java launcher. Their original APK bytes are pinned external inputs.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[2]
BUFFER = 4 * 1024 * 1024
MANIFEST = "assets/starpoint-personal-service-cdn/manifest.sha256"
SWF = "assets/worldflipper_android_release.swf"


def digest(path):
    with Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def check(path, size, sha):
    if path.stat().st_size != size or digest(path) != sha:
        raise ValueError(f"Input checksum mismatch: {path.name}")


def download(url, path, size, sha):
    if path.exists():
        check(path, size, sha)
        return
    temporary = path.with_suffix(path.suffix + ".partial")
    # GitHub CLI handles authenticated downloads on hosted runners. urllib is
    # also supported for public inputs when building locally.
    if os.environ.get("GH_TOKEN") and url.startswith("https://github.com/"):
        match = re.fullmatch(r"https://github.com/([^/]+/[^/]+)/releases/download/([^/]+)/([^/]+)", url)
        if not match:
            raise ValueError("Unexpected GitHub release URL")
        repo, tag, name = match.groups()
        subprocess.run(["gh", "release", "download", tag, "--repo", repo,
                        "--pattern", name, "--output", str(temporary), "--clobber"], check=True)
    else:
        with urllib.request.urlopen(url, timeout=120) as response, temporary.open("wb") as output:
            shutil.copyfileobj(response, output, BUFFER)
    check(temporary, size, sha)
    temporary.replace(path)


def inspector():
    spec = importlib.util.spec_from_file_location("bundle_inspector", ROOT / "scripts/protocol-lab/inspect-android-apk-bundle.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def fetch_baseline(directory, metadata):
    target = directory / metadata["file"]
    if target.exists():
        check(target, metadata["bytes"], metadata["sha256"])
        return target
    partial = target.with_suffix(".partial")
    with partial.open("wb") as result:
        for volume in metadata["volumes"]:
            path = directory / volume["file"]
            print("Downloading", path.name, flush=True)
            download(metadata["url"] + path.name, path, volume["bytes"], volume["sha256"])
            with path.open("rb") as source:
                shutil.copyfileobj(source, result, BUFFER)
            path.unlink()  # Only this build's verified temporary download.
    check(partial, metadata["bytes"], metadata["sha256"])
    partial.replace(target)
    return target


def patch_game(source, target, metadata):
    with zipfile.ZipFile(source) as old, zipfile.ZipFile(target, "w") as new:
        swf = old.read(SWF)
        if hashlib.sha256(swf).hexdigest() != metadata["swf_sha256"]:
            raise ValueError("Unexpected game SWF; refusing to apply an offset patch")
        patched = bytearray(swf)
        if patched[17644749] != 0x26:
            raise ValueError("Unexpected preload opcode")
        patched[17644749] = 0x27
        if hashlib.sha256(patched).hexdigest() != metadata["patched_swf_sha256"]:
            raise ValueError("Patched SWF checksum mismatch")
        for entry in old.infolist():
            if entry.filename.startswith("META-INF/"):
                continue
            new.writestr(entry, patched if entry.filename == SWF else old.read(entry))


def sign(unsigned, signed, args):
    aligned = unsigned.with_suffix(".aligned.apk")
    subprocess.run([str(args.build_tools / "zipalign"), "-f", "-p", "4", str(unsigned), str(aligned)], check=True)
    subprocess.run(["java", "-jar", str(args.build_tools / "lib/apksigner.jar"),
                    "sign", "--ks", str(args.keystore), "--ks-key-alias", args.alias,
                    "--ks-pass", "env:ANDROID_KEYSTORE_PASSWORD", "--key-pass", "env:ANDROID_KEYSTORE_PASSWORD",
                    "--v1-signing-enabled", "true", "--v2-signing-enabled", "true",
                    "--v3-signing-enabled", "false", "--v4-signing-enabled", "false",
                    "--out", str(signed), str(aligned)], check=True)
    verified = subprocess.check_output(["java", "-jar", str(args.build_tools / "lib/apksigner.jar"),
                                       "verify", "--print-certs", str(signed)], text=True)
    subprocess.run([str(args.build_tools / "zipalign"), "-c", "-p", "4", str(signed)], check=True)
    return re.search(r"Signer #1 certificate SHA-256 digest: ([0-9a-f]{64})", verified).group(1)


def comic_files(archive, metadata):
    with zipfile.ZipFile(archive) as source:
        if set(source.namelist()) != {row["path"] for row in metadata["files"]}:
            raise ValueError("Unexpected comic archive entries")
        for row in metadata["files"]:
            name = row["path"]
            if ".." in name.split("/") or not re.fullmatch(r"local-comics/(catalog.json|[01]/[0-9]+/(base|large|small).png)", name):
                raise ValueError("Unsafe comic archive path")
            data = source.read(name)
            if len(data) != row["bytes"] or hashlib.sha256(data).hexdigest() != row["sha256"]:
                raise ValueError(f"Comic checksum mismatch: {name}")
            yield row, data


class PartWriter:
    """Write release volumes directly, avoiding another 12 GB monolithic copy."""
    def __init__(self, directory, name, part_size=1024**3):
        self.directory, self.name, self.part_size = directory, name, part_size
        self.whole, self.total, self.parts = hashlib.sha256(), 0, []
        self.file = None

    def write(self, data):
        view = memoryview(data)
        while view:
            if self.file is None:
                self.part_name = f"{self.name}.part{len(self.parts)+1:03d}"
                self.file = (self.directory / self.part_name).open("xb")
                self.part_hash, self.part_bytes = hashlib.sha256(), 0
            block = view[:self.part_size - self.part_bytes]
            self.file.write(block)
            self.part_hash.update(block)
            self.whole.update(block)
            self.part_bytes += len(block)
            self.total += len(block)
            view = view[len(block):]
            if self.part_bytes == self.part_size:
                self.close_part()

    def close_part(self):
        if self.file:
            self.file.close()
            self.parts.append({"file":self.part_name, "index":len(self.parts)+1,
                               "bytes":self.part_bytes,"sha256":self.part_hash.hexdigest()})
            self.file = None


def write_bundle(base, baseline, comics, comic_meta, destination, name, metadata, source_commit, signer):
    module = inspector()
    location, upstream_manifest_sha = module.read_footer(baseline)
    upstream_entries = module.read_manifest(baseline, location, upstream_manifest_sha)
    module.verify_duplicate_eocd(baseline, location)
    with zipfile.ZipFile(base) as apk:
        manifest = apk.read(MANIFEST)
    writer = PartWriter(destination, name)
    try:
        with base.open("rb") as source:
            shutil.copyfileobj(source, writer, BUFFER)
        with baseline.open("rb") as source:
            source.seek(location["payload_offset"])
            # Verify every CDN file while copying, not only the outer APK hash.
            for expected, size, relative, _ in upstream_entries:
                remaining, sha = size, hashlib.sha256()
                while remaining:
                    data = source.read(min(BUFFER, remaining))
                    if not data:
                        raise ValueError("Truncated CDN payload")
                    sha.update(data)
                    writer.write(data)
                    remaining -= len(data)
                if sha.hexdigest() != expected:
                    raise ValueError(f"CDN checksum mismatch: {relative}")
        for _, data in comic_files(comics, comic_meta):
            writer.write(data)
        payload_length = writer.total - base.stat().st_size
        writer.write(struct.pack("<8sIIQQ32s", b"SPAPKBDL", 1, 1, base.stat().st_size,
                                 payload_length, hashlib.sha256(manifest).digest()))
        _, _, eocd = module.find_final_eocd(base)
        writer.write(eocd)
    finally:
        writer.close_part()
    artifact = {"name":"android", "platform":"Android", "file":name, "bytes":writer.total,
                "sha256":writer.whole.hexdigest(), "source_commit":source_commit,
                "signer_sha256":signer, "part_size":writer.part_size, "volumes":writer.parts,
                "base_bytes":base.stat().st_size, "base_sha256":digest(base),
                "manifest_sha256":hashlib.sha256(manifest).hexdigest(),
                "upstream_sha256":metadata["sha256"], "comic_archive_sha256":comic_meta["sha256"]}
    (destination / "starpoint-mobile-release.json").write_text(json.dumps({
        "schema":"starpoint-mobile-release/v1", "artifacts":[artifact]}, indent=2)+"\n", encoding="utf8")
    (destination / "SHA256SUMS.txt").write_text("".join(
        f"{p['sha256']}  {p['file']}\n" for p in writer.parts)+f"{artifact['sha256']}  {name}\n", encoding="utf8")
    shutil.copyfile(ROOT / "scripts/distribution/restore_release.py", destination / "restore_release.py")
    shutil.copyfile(ROOT / "deployment/distribution/RELEASE_NOTES.md", destination / "RELEASE_NOTES.md")
    print(json.dumps({"bytes":writer.total, "volumes":len(writer.parts), "sha256":artifact["sha256"]}), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--comics", type=Path)
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--build-tools", type=Path, required=True)
    parser.add_argument("--keystore", type=Path, required=True)
    parser.add_argument("--alias", default="local-repair")
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[a-zA-Z0-9._-]+", args.version):
        raise ValueError("Invalid version")
    args.work.mkdir(parents=True, exist_ok=True)
    args.output.mkdir(parents=True, exist_ok=False)
    metadata = json.loads((ROOT / "deployment/distribution/upstream-android.json").read_text())
    comic_meta = json.loads((ROOT / "deployment/distribution/comics.json").read_text(encoding="utf8"))
    baseline = args.baseline or fetch_baseline(args.work, metadata)
    check(baseline, metadata["bytes"], metadata["sha256"])
    comics = args.comics or args.work / "comics.zip"
    download(comic_meta["url"], comics, comic_meta["bytes"], comic_meta["sha256"])
    original = args.work / "original-base.apk"
    with baseline.open("rb") as source, original.open("wb") as target:
        remaining = metadata["base_bytes"]
        while remaining:
            data = source.read(min(BUFFER, remaining))
            if not data:
                raise ValueError("Truncated base APK")
            target.write(data)
            remaining -= len(data)
    check(original, metadata["base_bytes"], metadata["base_sha256"])
    game = args.work / "original-game.apk"
    with zipfile.ZipFile(original) as source:
        game.write_bytes(source.read("assets/starpoint-game.apk"))
        old_manifest = source.read(MANIFEST)
    if not old_manifest.endswith(b"\n"):
        raise ValueError("Unexpected manifest line ending")
    manifest = old_manifest + "".join(f"{r['sha256']}\t{r['bytes']}\t{r['path']}\n"
                                    for r, _ in comic_files(comics, comic_meta)).encode()
    unsigned_game, signed_game = args.work / "game-unsigned.apk", args.work / "game.apk"
    patch_game(game, unsigned_game, metadata)
    game_signer = sign(unsigned_game, signed_game, args)
    commit = subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip()
    unsigned_base, signed_base = args.work / "base-unsigned.apk", args.work / "base.apk"
    replacement = {"lib/arm64-v8a/libstarpoint_android_bridge.so":args.library.read_bytes(),
                   "assets/starpoint-game.apk":signed_game.read_bytes(),
                   "assets/starpoint-game-signers.sha256":(game_signer+"\n").encode(), MANIFEST:manifest}
    with zipfile.ZipFile(original) as source, zipfile.ZipFile(unsigned_base, "w") as target:
        if not replacement.keys() <= set(source.namelist()):
            raise ValueError("Missing required base APK entries")
        for entry in source.infolist():
            if not entry.filename.startswith("META-INF/"):
                target.writestr(entry, replacement.get(entry.filename, source.read(entry)))
        target.writestr("assets/somaur-distribution.json", json.dumps({
            "version":args.version,"source_commit":commit,"native_sha256":digest(args.library),
            "upstream_sha256":metadata["sha256"],"publisher":"Somaur"}))
    signer = sign(unsigned_base, signed_base, args)
    write_bundle(signed_base, baseline, comics, comic_meta, args.output,
                 f"StarpointCN-Somaur-{args.version}.apk", metadata, commit, signer)


if __name__ == "__main__":
    main()
