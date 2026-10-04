import hashlib
import json
from pathlib import Path
import tempfile
import struct
import unittest
import zipfile

from package_android import PartWriter, patch_game, write_bundle, inspector, SWF, MANIFEST
from restore_release import restore


class ReleaseTests(unittest.TestCase):
    def make_release(self, root):
        payload = bytes(range(256)) * 19 + b"last-part"
        writer = PartWriter(root, "game.apk", part_size=1024)
        for start in range(0, len(payload), 137):
            writer.write(payload[start:start + 137])
        writer.close_part()
        artifact = {"name":"android", "file":"game.apk", "bytes":writer.total,
                    "sha256":writer.whole.hexdigest(), "volumes":writer.parts}
        (root / "starpoint-mobile-release.json").write_text(json.dumps({"artifacts":[artifact]}))
        return payload

    def test_streamed_volumes_restore_exact_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            expected = self.make_release(root)
            self.assertEqual(restore(root).read_bytes(), expected)

    def test_corrupted_volume_never_becomes_final_apk(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.make_release(root)
            (root / "game.apk.part003").write_bytes(b"corrupted")
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                restore(root)
            self.assertFalse((root / "game.apk").exists())

    def test_unexpected_client_is_rejected_before_patching(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with zipfile.ZipFile(root / "game.apk", "w") as apk:
                apk.writestr(SWF, b"different version")
            with self.assertRaisesRegex(ValueError, "Unexpected game SWF"):
                patch_game(root / "game.apk", root / "patched.apk", {"swf_sha256":"0" * 64})

    def test_repacked_bundle_has_valid_manifest_payload_footer_and_eocd(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            payload = b"original-cdn-file"
            old_manifest = f"{hashlib.sha256(payload).hexdigest()}\t{len(payload)}\tpath\n".encode()
            baseline = root / "upstream.apk"
            with zipfile.ZipFile(baseline, "w") as apk:
                apk.writestr(MANIFEST, old_manifest)
            module = inspector()
            _, _, eocd = module.find_final_eocd(baseline)
            base_size = baseline.stat().st_size
            with baseline.open("ab") as output:
                output.write(payload)
                output.write(struct.pack("<8sIIQQ32s", b"SPAPKBDL", 1, 1, base_size,
                                         len(payload), hashlib.sha256(old_manifest).digest()))
                output.write(eocd)
            comic = b"test-comic"
            row = {"path":"local-comics/0/1/base.png", "bytes":len(comic),
                   "sha256":hashlib.sha256(comic).hexdigest()}
            new_manifest = old_manifest + f"{row['sha256']}\t{row['bytes']}\t{row['path']}\n".encode()
            base = root / "base.apk"
            with zipfile.ZipFile(base, "w") as apk:
                apk.writestr(MANIFEST, new_manifest)
            comics = root / "comics.zip"
            with zipfile.ZipFile(comics, "w") as archive:
                archive.writestr(row["path"], comic)
            write_bundle(base, baseline, comics, {"files":[row], "sha256":"comic-hash"},
                         root, "release.apk", {"sha256":"baseline-hash"}, "source-commit", "signer")
            combined = restore(root)
            location, sha = module.read_footer(combined)
            entries = module.read_manifest(combined, location, sha)
            module.verify_payload(combined, entries)
            module.verify_duplicate_eocd(combined, location)
            self.assertEqual([e[2] for e in entries], ["path", row["path"]])


if __name__ == "__main__":
    unittest.main()
