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
        with tempfile.TemporaryDirectory() as folder:
            with patch.object(probe.os, "link", side_effect=lambda a, b: b.write_bytes(a.read_bytes())):
                report = probe.probe(Path(folder), ["hardlink"], "intentionally-broken-copy")
            self.assertEqual(report["counts"], {"FAIL": 1})

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

    def test_report_never_overwrites_existing_file(self):
        with tempfile.TemporaryDirectory() as folder:
            report = Path(folder) / "report.json"
            report.write_bytes(b"keep")
            result = probe.main(["--root", folder, "--cases", "io", "--report", str(report)])
            self.assertEqual(result, 2)
            self.assertEqual(report.read_bytes(), b"keep")


if __name__ == "__main__":
    unittest.main()
