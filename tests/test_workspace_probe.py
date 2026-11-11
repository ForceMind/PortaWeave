"""Harness tests. Passing these does not validate an unimplemented filesystem."""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from tools import workspace_probe as probe


class ProbeTests(unittest.TestCase):
    def test_core_cases_and_cleanup(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            sentinel = root / "user-file"
            sentinel.write_bytes(b"untouched")
            report = probe.probe(root, ["io", "mmap", "replace", "names", "exclusive", "sqlite"], "host")
            self.assertTrue(report["applicable_selected_cases_passed"], report)
            self.assertFalse(report["product_compatibility_certified"])
            self.assertFalse(report["mount_identity_verified"])
            self.assertEqual(list(root.iterdir()), [sentinel])
            self.assertEqual(sentinel.read_bytes(), b"untouched")

    def test_hardlink_requires_shared_identity(self):
        def copy(source, alias):
            alias.write_bytes(source.read_bytes())

        with tempfile.TemporaryDirectory() as folder:
            if probe.os.name == "nt":
                from tools import windows_semantics
                replacement = patch.object(windows_semantics.Api, "create_hard_link", side_effect=copy)
            else:
                replacement = patch.object(probe.os, "link", side_effect=copy)
            with replacement:
                report = probe.probe(Path(folder), ["hardlink"], "intentionally-broken-copy")
            self.assertEqual(report["counts"], {"FAIL": 1})
            self.assertIn("identity", report["results"][0]["detail"])

    def test_unavailable_tool_is_not_a_pass(self):
        with tempfile.TemporaryDirectory() as folder:
            with patch.object(probe.shutil, "which", return_value=None):
                report = probe.probe(Path(folder), ["git"], "missing-tool")
            self.assertEqual(report["counts"], {"NOT_RUN": 1})
            self.assertFalse(report["applicable_selected_cases_passed"])

    def test_reject_backing_container_and_descendant(self):
        with tempfile.TemporaryDirectory(suffix=".pwvol") as folder:
            root = Path(folder)
            child = root / "segments"
            child.mkdir()
            for path in (root, child):
                with self.assertRaises(ValueError):
                    probe.probe(path, ["io"], "invalid")

    def test_reject_unknown_empty_duplicate(self):
        with tempfile.TemporaryDirectory() as folder:
            for cases in (["unknown"], [], ["io", "io"]):
                with self.assertRaises(ValueError):
                    probe.probe(Path(folder), cases, "invalid")

    def test_reject_renamed_backing_layout_without_mutation(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "format.json").write_bytes(b"do not parse me")
            (root / "catalog.sqlite").write_bytes(b"do not open me")
            child = root / "segments"
            child.mkdir()
            for path in (root, child):
                with self.assertRaises(ValueError):
                    probe.probe(path, ["windows-share"], "raw-renamed")
            self.assertEqual((root / "format.json").read_bytes(), b"do not parse me")
            self.assertEqual((root / "catalog.sqlite").read_bytes(), b"do not open me")
            self.assertEqual(set(root.iterdir()), {root / "format.json", root / "catalog.sqlite", child})
            self.assertEqual(list(child.iterdir()), [])

    def test_reject_current_core_formats_before_creating_any_fixture(self):
        # These are the markers actually written by Volume::create*; the
        # design's format.json-only fixture missed all existing core formats.
        for marker in (b"experimental-m1a-block-v1",
                       b"experimental-m1h-metadata-v2",
                       b"experimental-m1o-directory-v3",
                       b"unknown-or-damaged-marker"):
            with self.subTest(marker=marker), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                (root / "format.txt").write_bytes(marker)
                (root / "catalog.sqlite").write_bytes(b"never open this catalog")
                (root / "catalog.sqlite-wal").write_bytes(b"preserve WAL")
                child = root / "segments" / "nested"
                child.mkdir(parents=True)
                (child / "0000000000000001.seg").write_bytes(b"preserve segment")
                before = {p.relative_to(root): p.read_bytes()
                          for p in root.rglob("*") if p.is_file()}
                names = {p.relative_to(root) for p in root.rglob("*")}
                # The refusal must precede private directory creation, not
                # merely fail a later case and then clean its files up.
                with patch.object(probe.tempfile, "TemporaryDirectory",
                                  side_effect=AssertionError("fixture admitted")):
                    for target in (root, root / "segments", child):
                        with self.assertRaises(ValueError):
                            probe.probe(target, ["io"], "renamed-current-core")
                self.assertEqual(names, {p.relative_to(root) for p in root.rglob("*")})
                self.assertEqual(before, {p.relative_to(root): p.read_bytes()
                                         for p in root.rglob("*") if p.is_file()})

    def test_cli_rejects_current_raw_layout_without_emitting_report(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "format.txt").write_bytes(b"experimental-m1a-block-v1")
            (root / "catalog.sqlite").write_bytes(b"preserve")
            report = root / "must-not-exist.json"
            self.assertEqual(probe.main(["--root", str(root), "--cases", "io",
                                         "--report", str(report)]), 2)
            self.assertFalse(report.exists())

    @unittest.skipIf(probe.os.name == "nt", "Real Win32 cases are run by the host baseline")
    def test_windows_cases_never_pass_on_other_platforms(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            selected = [name for name in probe.CASES if name.startswith("windows-")]
            report = probe.probe(root, selected, "not-windows")
            self.assertEqual(report["counts"], {"NOT_APPLICABLE": 4})
            self.assertFalse(report["mount_identity_verified"])
            self.assertFalse(report["product_compatibility_certified"])
            self.assertEqual(list(root.iterdir()), [])

    def test_report_never_overwrites_existing_file(self):
        with tempfile.TemporaryDirectory() as folder:
            report = Path(folder) / "report.json"
            report.write_bytes(b"keep")
            result = probe.main(["--root", folder, "--cases", "io", "--report", str(report)])
            self.assertEqual(result, 2)
            self.assertEqual(report.read_bytes(), b"keep")


if __name__ == "__main__":
    unittest.main()
