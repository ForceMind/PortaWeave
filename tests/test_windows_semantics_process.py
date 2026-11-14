"""Real subprocess protocol failures and actual Win32 integration when present."""
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

from tools import windows_semantics as win
from tools import workspace_probe as probe


# This child is deliberately independent of the implementation's encoder.
# It exercises actual pipes, exit codes, deadlines and process cleanup on all
# three CI hosts, without pretending to emulate any Win32 filesystem API.
CHILD = r'''
import json, os, sys, time
mode = sys.argv[1]
protocol = "portaweave-win32-probe-v1"
def reply(n, op, **extra):
    data = {"protocol": protocol, "sequence": n, "pid": os.getpid(), "operation": op}
    data.update(extra)
    print(json.dumps(data), flush=True)
if mode == "no-ready":
    time.sleep(60)
reply(0, "ready", result={})
for line in sys.stdin:
    request = json.loads(line)
    n, op = request["sequence"], request["operation"]
    if op == "quit":
        reply(n, op, result={})
        if mode == "quit-hang": time.sleep(60)
        if mode == "quit-extra": print("unexpected trailer", flush=True)
        raise SystemExit(7 if mode == "quit-nonzero" else 0)
    if mode == "hang": time.sleep(60)
    if mode == "early-eof": raise SystemExit(3)
    if mode == "wrong-pid":
        print(json.dumps({"protocol": protocol, "sequence": n, "pid": os.getppid(),
                          "operation": op, "result": {}}), flush=True)
    elif mode == "wrong-sequence": reply(n + 1, op, result={})
    elif mode == "unknown-field": reply(n, op, result={}, unrequested=True)
    elif mode == "duplicate-key":
        print('{"protocol":"%s","sequence":%d,"sequence":%d,"pid":%d,"operation":"%s","result":{}}'
              % (protocol, n, n, os.getpid(), op), flush=True)
    elif mode == "oversized": print("x" * 5000, flush=True)
    elif mode == "malformed-json": print("not json", flush=True)
    elif mode == "missing-newline":
        sys.stdout.write('{}'); sys.stdout.flush(); raise SystemExit(0)
    elif mode == "child-error": reply(n, op, error="deliberate native failure")
    else: reply(n, op, result={"identity": [123, "0123456789abcdef0123456789abcdef"]})
'''


class ProcessProtocolTests(unittest.TestCase):
    def child(self, mode="good", **options):
        return win._ProbeChild([sys.executable, "-I", "-u", "-c", CHILD, mode], **options)

    def assert_reaped(self, child):
        self.assertTrue(child.closed)
        self.assertIsNotNone(child.process.poll())
        self.assertFalse(child.thread and child.thread.is_alive())
        self.assertTrue(child.process.stdin.closed)
        self.assertTrue(child.process.stdout.closed)

    def test_independent_process_and_clean_protocol_exit(self):
        child = self.child()
        with child:
            self.assertNotEqual(child.process.pid, os.getpid())
            win._identity_reply(child.request("read"),
                                (123, "0123456789abcdef0123456789abcdef"))
        self.assertEqual(child.process.returncode, 0)
        self.assert_reaped(child)
        with self.assertRaisesRegex(RuntimeError, "cannot be reused"):
            child.__enter__()

    def test_corrupt_or_failed_child_never_becomes_pass(self):
        for mode in ("wrong-pid", "wrong-sequence", "unknown-field", "duplicate-key",
                     "oversized", "malformed-json", "missing-newline", "child-error", "early-eof"):
            with self.subTest(mode=mode):
                child = self.child(mode)
                with self.assertRaises(RuntimeError):
                    with child:
                        child.request("read")
                self.assert_reaped(child)

    def test_wrong_file_identity_refuses_a_successful_child_reply(self):
        child = self.child()
        with self.assertRaisesRegex(RuntimeError, "file identity mismatch"):
            with child:
                win._identity_reply(child.request("read"), (123, "f" * 32))
        self.assert_reaped(child)

    def test_reply_timeout_kills_and_joins_only_owned_child(self):
        child = self.child("hang")
        with self.assertRaisesRegex(RuntimeError, "reply timed out"):
            with child:
                child.message_timeout = 0.15  # startup has its normal 10-second allowance
                child.request("read")
        self.assert_reaped(child)

    def test_startup_timeout_also_reaps(self):
        child = self.child("no-ready", message_timeout=0.15)
        with self.assertRaisesRegex(RuntimeError, "reply timed out"):
            with child:
                self.fail("Missing handshake was accepted")
        self.assert_reaped(child)

    def test_quit_requires_zero_exit_eof_and_timely_exit(self):
        for mode in ("quit-nonzero", "quit-extra", "quit-hang"):
            with self.subTest(mode=mode):
                child = self.child(mode)
                with self.assertRaises(RuntimeError):
                    with child:
                        child.request("read")
                        if mode == "quit-hang":
                            child.message_timeout = 0.15
                self.assert_reaped(child)

    def test_parent_failure_reaps_blocked_child(self):
        child = self.child()
        with self.assertRaisesRegex(ValueError, "parent failed"):
            with child:
                raise ValueError("parent failed")
        self.assert_reaped(child)

    def test_protocol_failure_is_reported_and_private_fixture_removed(self):
        child = self.child("child-error")

        def failed_case(path):
            (path / "private").write_bytes(b"fixture")
            with child:
                child.request("read")
            return "must not be reached"

        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            sentinel = root / "user-data"
            sentinel.write_bytes(b"preserve")
            with patch.dict(probe.CASES, {"child-protocol-failure": failed_case}):
                result = probe.probe(root, ["child-protocol-failure"], "failure-regression")
            self.assertEqual(result["counts"], {"FAIL": 1})
            self.assertFalse(result["applicable_selected_cases_passed"])
            self.assertFalse(result["product_compatibility_certified"])
            self.assertEqual(list(root.iterdir()), [sentinel])
            self.assertEqual(sentinel.read_bytes(), b"preserve")
        self.assert_reaped(child)

    def test_production_child_refuses_an_arbitrary_directory_without_mutation(self):
        script = Path(win.__file__).resolve().with_name("windows_semantics_child.py")
        with tempfile.TemporaryDirectory() as folder:
            sentinel = Path(folder) / "locks.bin"
            sentinel.write_bytes(b"preserve")
            child = win._ProbeChild([sys.executable, "-I", "-u", str(script), "locking", folder])
            with self.assertRaises(RuntimeError):
                with child:
                    self.fail("Unscoped child fixture accepted")
            self.assertEqual(list(Path(folder).iterdir()), [sentinel])
            self.assertEqual(sentinel.read_bytes(), b"preserve")
        self.assert_reaped(child)

    @unittest.skipUnless(os.name == "nt", "Actual Win32 cross-process integration requires Windows")
    def test_real_windows_share_and_lock_keep_uncertified_scope_and_clean_up(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            sentinel = root / "user-data"
            sentinel.write_bytes(b"preserve")
            report = probe.probe(root, ["windows-share", "windows-lock"], "host-regression")
            self.assertEqual(report["counts"], {"PASS": 2}, report)
            self.assertIn("WIN32_CROSS_PROCESS_SHARE_OK", report["results"][0]["detail"])
            self.assertIn("WIN32_CROSS_PROCESS_LOCK_OK", report["results"][1]["detail"])
            self.assertFalse(report["mount_identity_verified"])
            self.assertFalse(report["product_compatibility_certified"])
            self.assertEqual(list(root.iterdir()), [sentinel])
            self.assertEqual(sentinel.read_bytes(), b"preserve")


if __name__ == "__main__":
    unittest.main()
