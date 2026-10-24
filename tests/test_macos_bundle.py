"""Regression for dependency validation of real unsigned Xcode artifacts."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "macos_bundle", Path(__file__).parents[1] / "ci/verify-macos-bundle.py"
)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class DependencyEvidenceTests(unittest.TestCase):
    def test_binary_header_in_target_is_not_a_dynamic_dependency(self):
        libraries = module.dynamic_dependencies(
            "/scratch/target/Release/PortaWeave:\n"
            "\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)\n"
        )
        self.assertEqual(libraries, ["/usr/lib/libSystem.B.dylib"])

    def test_build_tree_and_unbundled_native_dylib_are_rejected(self):
        for library in ["/scratch/target/release/other.dylib", "@rpath/libportaweave_native.dylib"]:
            with self.subTest(library=library), self.assertRaises(ValueError):
                module.dynamic_dependencies(f"/scratch/PortaWeave:\n\t{library} (compatibility version 1.0.0)\n")

    def test_missing_otool_header_is_not_passed_as_empty_dependencies(self):
        for output in ["", "not an otool report"]:
            with self.assertRaises(ValueError):
                module.dynamic_dependencies(output)
