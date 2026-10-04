"""Verify and join release volumes. Python 3.11+; no third-party modules."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import urllib.request


def restore(directory, release_url=None):
    manifest = directory / "starpoint-mobile-release.json"
    if not manifest.exists() and release_url:
        urllib.request.urlretrieve(release_url.rstrip("/") + "/" + manifest.name, manifest)
    data = json.loads(manifest.read_text(encoding="utf-8-sig"))
    artifact = next(a for a in data["artifacts"] if a["name"] == "android")
    names = [artifact["file"]] + [v["file"] for v in artifact["volumes"]]
    if any(not re.fullmatch(r"[A-Za-z0-9._-]+", n) or n in (".", "..") for n in names):
        raise ValueError("Unsafe release filename")
    output = directory / artifact["file"]
    if output.exists():
        raise FileExistsError(f"Output already exists: {output}")
    temporary = output.with_suffix(output.suffix + ".partial")
    whole = hashlib.sha256()
    size = 0
    with temporary.open("xb") as result:
        for index, volume in enumerate(artifact["volumes"], 1):
            if volume["index"] != index:
                raise ValueError("Release volume order is invalid")
            part = directory / volume["file"]
            if not part.exists() and release_url:
                print("Downloading", part.name, flush=True)
                download = part.with_suffix(part.suffix + ".partial")
                urllib.request.urlretrieve(release_url.rstrip("/") + "/" + part.name, download)
                with download.open("rb") as source:
                    sha = hashlib.file_digest(source, "sha256").hexdigest()
                if download.stat().st_size != volume["bytes"] or sha != volume["sha256"]:
                    raise ValueError(f"Download checksum mismatch: {part.name}")
                download.replace(part)
            sha = hashlib.sha256()
            part_size = 0
            with part.open("rb") as source:
                for block in iter(lambda: source.read(4 * 1024 * 1024), b""):
                    result.write(block)
                    sha.update(block)
                    whole.update(block)
                    part_size += len(block)
            if part_size != volume["bytes"] or sha.hexdigest() != volume["sha256"]:
                raise ValueError(f"Volume checksum mismatch: {part.name}")
            size += part_size
            print("Verified", part.name, flush=True)
    if size != artifact["bytes"] or whole.hexdigest() != artifact["sha256"]:
        raise ValueError("Combined APK checksum mismatch")
    temporary.replace(output)
    print("Ready:", output)
    return output


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument("--release-url", help="Optional GitHub releases/download/TAG URL for missing parts")
    args = parser.parse_args()
    args.directory.mkdir(parents=True, exist_ok=True)
    restore(args.directory, args.release_url)
