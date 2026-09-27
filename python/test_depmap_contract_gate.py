import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "depmap_contract_gate.py"
FIXTURE = ROOT / "scripts" / "fixtures" / "depmap-status-compatible.json"
SPEC = importlib.util.spec_from_file_location("depmap_contract_gate", SCRIPT)
gate = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = gate
SPEC.loader.exec_module(gate)


def assessment(version=13, *, usable=True):
    identity = "fixture" if usable else "catalog-missing"
    return gate.assess_status(
        {
            "evidence": {
                "query_contract_version": version,
                "server_build_identity": "server-fixture",
                "capability_catalog_digest": "sha256:fixture",
                "catalog_build_identity": identity,
            }
        },
        gate.ContractRange(13, 14),
    )


def classify(
    paths,
    *,
    body="Closes #139",
    base_client=(13, 13),
    head_client=(13, 13),
    base_provider=13,
    head_provider=13,
    live=None,
):
    return gate.classify_pr(
        body=body,
        changed_paths=paths,
        base_client=gate.ContractRange(*base_client),
        head_client=gate.ContractRange(*head_client),
        base_provider_version=base_provider,
        head_provider_version=head_provider,
        live_assessment=live,
    )


class DepMapContractGateTests(unittest.TestCase):
    def test_parses_contract_versions_without_hardcoding_current_value(self):
        self.assertEqual(
            gate.parse_client_range(
                "const DEPMAP_QUERY_CONTRACT_MIN: u64 = 17;\n"
                "const DEPMAP_QUERY_CONTRACT_MAX: u64 = 18;\n"
            ),
            gate.ContractRange(17, 18),
        )
        self.assertEqual(gate.parse_provider_version("QUERY_CONTRACT_VERSION = 18"), 18)
        with self.assertRaises(gate.GateError):
            gate.parse_client_range(
                "const DEPMAP_QUERY_CONTRACT_MIN: u64 = 19;\n"
                "const DEPMAP_QUERY_CONTRACT_MAX: u64 = 18;\n"
            )

    def test_issue_linkage_and_path_normalization(self):
        for body in (
            "Closes #139",
            "Fixes owner/repo#139",
            "Refs: #147",
            "Related to #139",
        ):
            self.assertTrue(gate.pr_links_issue(body))
        self.assertFalse(gate.pr_links_issue("mentions #139 without linkage"))
        report = classify([r"services\depmap_api\query.py"])
        self.assertEqual(report.classification, "provider_backward_compatible")

    def test_non_issue_and_local_only_changes_use_ordinary_ci(self):
        non_issue = classify([gate.CLIENT_CONTRACT_PATH], body="context only")
        self.assertEqual(non_issue.classification, "not_issue_fix")
        self.assertTrue(non_issue.merge_allowed)
        local = classify(["docs/depmap-release-gate.md"])
        self.assertEqual(local.classification, "local_only")
        self.assertTrue(local.merge_allowed)

    def test_current_client_and_backward_compatible_provider_are_offline_gated(self):
        client = classify([gate.CLIENT_CONTRACT_PATH])
        self.assertEqual(client.classification, "client_current_contract")
        self.assertTrue(client.merge_allowed)
        provider = classify(["services/depmap_mcp/server.py"])
        self.assertEqual(provider.classification, "provider_backward_compatible")
        self.assertTrue(provider.merge_allowed)
        self.assertEqual(provider.live_gate, "deployment_and_release")

    def test_expand_then_deploy_sequence_avoids_live_deadlock(self):
        client = classify(
            [gate.CLIENT_CONTRACT_PATH], head_client=(13, 14)
        )
        self.assertEqual(client.classification, "contract_expansion_client")
        self.assertTrue(client.merge_allowed)
        provider = classify(
            ["services/depmap_api/app.py"],
            base_client=(13, 14),
            head_client=(13, 14),
            head_provider=14,
        )
        self.assertEqual(provider.classification, "contract_expansion_provider")
        self.assertTrue(provider.merge_allowed)
        too_early = classify(
            ["services/depmap_api/app.py"], head_provider=14
        )
        self.assertFalse(too_early.merge_allowed)

    def test_provider_and_client_boundary_changes_must_be_split(self):
        report = classify(
            [gate.CLIENT_CONTRACT_PATH, "services/depmap_api/app.py"],
            head_client=(13, 14),
            head_provider=14,
        )
        self.assertEqual(report.classification, "mixed_provider_client_boundary")
        self.assertFalse(report.merge_allowed)

    def test_compatible_logic_changes_in_both_areas_do_not_fake_boundary_move(self):
        report = classify(
            [gate.CLIENT_CONTRACT_PATH, "services/depmap_mcp/server.py"]
        )
        self.assertNotEqual(report.classification, "mixed_provider_client_boundary")
        self.assertTrue(report.merge_allowed)

    def test_contract_removal_requires_fresh_compatible_live_assessment(self):
        without_live = classify(
            [gate.CLIENT_CONTRACT_PATH],
            base_client=(13, 14),
            head_client=(14, 14),
            base_provider=14,
            head_provider=14,
        )
        self.assertFalse(without_live.merge_allowed)
        with_live = classify(
            [gate.CLIENT_CONTRACT_PATH],
            base_client=(13, 14),
            head_client=(14, 14),
            base_provider=14,
            head_provider=14,
            live=assessment(14),
        )
        self.assertTrue(with_live.merge_allowed)
        stale_repository = classify(
            [gate.CLIENT_CONTRACT_PATH],
            base_client=(13, 14),
            head_client=(14, 14),
            base_provider=13,
            head_provider=13,
            live=assessment(14),
        )
        self.assertFalse(stale_repository.merge_allowed)
        self.assertTrue(
            any("repository provider" in reason for reason in stale_repository.reasons)
        )

    def test_pr_paths_use_merge_base_and_contract_uses_effective_merge(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            event = Path(temp_dir) / "event.json"
            event.write_text(
                json.dumps({"pull_request": {"body": "Closes #139"}}),
                encoding="utf-8",
            )

            def fake_git(_repo, *args):
                if args == ("merge-base", "base", "head"):
                    return "common\n"
                if args == ("diff", "--name-only", "common", "head"):
                    return "docs/depmap-deployment-contract.md\n"
                self.fail(f"unexpected git call: {args}")

            def fake_file(_repo, revision, relative):
                if relative == gate.CLIENT_CONTRACT_PATH:
                    version = 13 if revision == "base" else 14
                    return (
                        f"const DEPMAP_QUERY_CONTRACT_MIN: u64 = {version};\n"
                        f"const DEPMAP_QUERY_CONTRACT_MAX: u64 = {version};\n"
                    )
                version = 13 if revision == "base" else 14
                return f"QUERY_CONTRACT_VERSION = {version}\n"

            with mock.patch.object(gate, "_git", side_effect=fake_git), mock.patch.object(
                gate, "_git_file", side_effect=fake_file
            ):
                report = gate.inspect_pr(
                    ROOT,
                    event,
                    "base",
                    "head",
                    effective="merge",
                )
        self.assertEqual(report.classification, "local_only")
        self.assertEqual(report.head_client, gate.ContractRange(14, 14))
        self.assertEqual(report.head_provider_version, 14)

    def test_status_requires_version_and_all_usable_identities(self):
        document = json.loads(FIXTURE.read_text(encoding="utf-8"))
        self.assertTrue(
            gate.assess_status(document, gate.ContractRange(13, 13)).compatible
        )
        self.assertFalse(assessment(15).compatible)
        self.assertFalse(assessment(13, usable=False).compatible)
        for key in (
            "server_build_identity",
            "capability_catalog_digest",
            "catalog_build_identity",
        ):
            broken = json.loads(FIXTURE.read_text(encoding="utf-8"))
            del broken["evidence"][key]
            self.assertFalse(
                gate.assess_status(broken, gate.ContractRange(13, 13)).compatible
            )

    def test_attestation_rejects_stale_and_future_observations(self):
        now = datetime(2026, 9, 27, tzinfo=timezone.utc)
        for observed in (now - timedelta(hours=73), now + timedelta(minutes=6)):
            document = {
                "schema": gate.ATTESTATION_SCHEMA,
                "observed_at": observed.isoformat(),
                "status": json.loads(FIXTURE.read_text(encoding="utf-8")),
            }
            with self.assertRaises(gate.GateError):
                gate.require_fresh_attestation(document, now=now, max_age_hours=72)

    def test_compare_command_is_offline_and_serializes_nested_ranges(self):
        completed = subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "compare",
                "--repo-root",
                str(ROOT),
                "--status-file",
                str(FIXTURE),
            ],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        payload = json.loads(completed.stdout)
        self.assertTrue(payload["compatible"])
        self.assertEqual(payload["client_minimum"], 13)

    def test_attestation_projection_drops_unrelated_or_path_like_status(self):
        status = json.loads(FIXTURE.read_text(encoding="utf-8"))
        status["evidence"]["debug_path"] = r"C:\\server\\private\\index.sqlite"
        status["provenance"] = {"uri": "file:///srv/private/index.sqlite"}
        projected = gate.portable_attestation_status(status)
        self.assertEqual(
            set(projected["evidence"]),
            {
                "query_contract_version",
                "server_build_identity",
                "capability_catalog_digest",
                "catalog_build_identity",
            },
        )
        self.assertNotIn("private", json.dumps(projected))

    def test_workflows_wire_offline_pr_and_fresh_release_gates(self):
        test_workflow = (ROOT / ".github" / "workflows" / "test.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("depmap-contract-pr-gate:", test_workflow)
        self.assertIn("if: github.event_name == 'pull_request'", test_workflow)
        self.assertIn("fetch-depth: 0", test_workflow)
        self.assertIn("depmap_contract_gate.py pr", test_workflow)
        self.assertIn('--effective "$GITHUB_SHA"', test_workflow)
        self.assertNotIn("depmap_contract_gate.py probe", test_workflow)

        release_workflow = (
            ROOT / ".github" / "workflows" / "release-create.yml"
        ).read_text(encoding="utf-8")
        self.assertIn("depmap_contract_gate.py compare", release_workflow)
        self.assertIn(".github/depmap-live-contract.json", release_workflow)
        self.assertIn("--require-attestation", release_workflow)
        self.assertIn("--max-age-hours 72", release_workflow)


if __name__ == "__main__":
    unittest.main()
