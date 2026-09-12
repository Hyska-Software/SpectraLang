"""Focused contract tests for the R-3007 auditor."""

from __future__ import annotations

import copy
import sys
import tomllib
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import validate_r3007_stdlib_contract as audit  # noqa: E402


class R3007ContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.manifest = tomllib.loads(
            (audit.ROOT / "scripts" / "stdlib_contract.toml").read_text(encoding="utf-8")
        )

    def test_manifest_is_valid(self) -> None:
        self.assertEqual(audit.validate_manifest(audit.ROOT, self.manifest), [])

    def test_duplicate_namespace_is_rejected(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["namespace"].append(copy.deepcopy(manifest["namespace"][0]))
        self.assertIn("manifest contains duplicate namespace prefixes", audit.validate_manifest(audit.ROOT, manifest))

    def test_invalid_owner_and_classification_are_rejected(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["namespace"][0]["owner"] = "unknown"
        manifest["namespace"][0]["classification"] = "mock"
        errors = audit.validate_manifest(audit.ROOT, manifest)
        self.assertTrue(any("invalid owner" in error for error in errors))
        self.assertTrue(any("invalid classification" in error for error in errors))

    def test_missing_roadmap_reference_is_rejected(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["namespace"][0]["roadmap"] = "R-9999"
        self.assertTrue(any("missing roadmap item" in error for error in audit.validate_manifest(audit.ROOT, manifest)))

    def test_missing_source_and_probe_are_rejected(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["sources"]["runtime"] = ["does-not-exist.rs"]
        manifest["probe"][0]["path"] = "does-not-exist.spectra"
        errors = audit.validate_manifest(audit.ROOT, manifest)
        self.assertTrue(any("probe path is missing" in error for error in errors))
        self.assertTrue(any("source category has no files" in error for error in errors))

    def test_rule_overrides_namespace(self) -> None:
        contract = audit.classification_for("std.serve.server_new", self.manifest)
        self.assertEqual(contract["classification"], "incomplete")
        self.assertEqual(contract["roadmap"], "R-3001")

    def test_cross_source_gaps_are_blocking(self) -> None:
        inventory = audit.SourceInventory(
            symbols={
                "std.math.abs": audit.SymbolEvidence(sources={"semantic", "runtime"}, semantic_declared=True, runtime_registered=True, lowering_modes={"explicit_lowering"}),
                "std.math.host_only": audit.SymbolEvidence(sources={"runtime"}, runtime_registered=True),
                "std.math.no_lowering": audit.SymbolEvidence(sources={"semantic"}, semantic_declared=True),
                "std.math.lowering_only": audit.SymbolEvidence(sources={"lowering"}, lowering_modes={"explicit_lowering"}),
            },
            files={category: [] for category in audit.SOURCE_KEYS},
            signals=[],
        )
        report = audit.build_report(
            audit.ROOT,
            self.manifest,
            inventory,
            [{"id": "std-core", "path": self.manifest["probe"][0]["path"], "status": "passed", "exit_code": 0, "command": []}],
        )
        kinds = {blocker["kind"] for blocker in report["blockers"]}
        self.assertIn("divergence_without_follow_up", kinds)

    def test_source_extractors_capture_arrays_and_registered_host_calls(self) -> None:
        symbols = {}
        audit.semantic_inventory(
            'fn make_std_demo() { let functions = [("from_array", vec![])]; exports.functions.insert("from_insert".to_string(), pub_fn(vec![], Type::Int)); }',
            symbols,
        )
        self.assertTrue(symbols["std.demo.from_array"].semantic_declared)
        self.assertTrue(symbols["std.demo.from_insert"].semantic_declared)
        runtime = {}
        audit.runtime_inventory(
            'const DEMO: &str = "spectra.std.demo.from_insert"; '
            'register_host_function(DEMO, demo); '
            'register_host_function("spectra.std.demo.literal", demo);',
            "runtime",
            runtime,
        )
        self.assertTrue(runtime["std.demo.from_insert"].runtime_registered)
        self.assertTrue(runtime["std.demo.literal"].runtime_registered)

    def test_multiline_modules_and_generated_numeric_functions_are_discovered(self) -> None:
        symbols = {}
        audit.semantic_inventory(
            'registry.register_module(\n'
            '    "std.compat.collections".to_string(),\n'
            '    make_std_compat_collections(),\n'
            ');\n'
            'fn make_std_numeric() {\n'
            '    for op in ["add", "sub", "mul"] {\n'
            '        format!("wrapping_{op}_{name}");\n'
            '    }\n'
            '}',
            symbols,
        )
        self.assertEqual(symbols["std.compat.collections"].kind, "module")
        self.assertIn("std.numeric.wrapping_add_i8", symbols)
        self.assertNotIn("std.numeric.i8", symbols)

    def test_record_declarations_are_types_not_functions(self) -> None:
        symbols = {}
        audit.semantic_inventory(
            'pub const TYPES: &[(&str, &str)] = &[("std.time.Duration", "record Duration")];',
            symbols,
        )
        self.assertEqual(symbols["std.time.Duration"].kind, "type")

    def test_lowering_modes_are_distinguished(self) -> None:
        symbols = {}
        generic, api = audit.lowering_inventory(
            'fn lookup_std_host_function() {} fn lookup_std_api_host_function() {} "spectra.std.math.abs" "spectra.api.http.method_get"',
            symbols,
        )
        self.assertTrue(generic)
        self.assertTrue(api)
        self.assertIn("explicit_lowering", symbols["std.math.abs"].lowering_modes)
        self.assertIn("api_external_lowering", symbols["std.api.http.method_get"].lowering_modes)

    def test_probe_coverage_is_pattern_based(self) -> None:
        self.assertEqual(
            [p["id"] for p in audit.probe_matches("std.tensor.arange", self.manifest)],
            ["tensor-kernels", "tensor-ir-device-lowering"],
        )
        self.assertEqual([p["id"] for p in audit.probe_matches("std.api.http.method_get", self.manifest)], ["api-http", "api-conformance"])

    def test_postgres_probe_can_use_a_container_psql(self) -> None:
        probe = next(item for item in self.manifest["probe"] if item["id"] == "api-postgres-driver")
        command = audit.build_probe_command(
            audit.ROOT / "target" / "debug" / "spectralang.exe",
            probe,
            "spectralang-stability-postgres16",
        )
        self.assertEqual(command[-2:], ["--version-probe-docker-container", "spectralang-stability-postgres16"])

    def test_canonicalizes_legacy_spectra_prefixes(self) -> None:
        self.assertEqual(audit.canonical_symbol("spectra.std.math.abs"), "std.math.abs")
        self.assertEqual(audit.canonical_symbol("spectra.api.http.method_get"), "std.api.http.method_get")

    def test_report_exposes_catalog_migration_coverage(self) -> None:
        inventory = audit.discover_sources(audit.ROOT, self.manifest)
        report = audit.build_report(audit.ROOT, self.manifest, inventory, [])
        coverage = report["catalog_coverage"]
        self.assertEqual(coverage["status"], "complete")
        self.assertEqual(coverage["missing_symbols"], [])
        self.assertGreaterEqual(coverage["catalog_symbol_count"], coverage["inventory_symbol_count"])
        self.assertFalse(any(item["kind"] == "catalog_migration_gap" for item in report["warnings"]))


if __name__ == "__main__":
    unittest.main()
