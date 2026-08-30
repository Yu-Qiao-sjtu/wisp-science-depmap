import json
import os
import tempfile
import unittest
from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq
from fastapi.testclient import TestClient

from services.depmap_api.app import (
    CANONICAL_LINEAGES,
    CHINESE_LINEAGE_ALIASES,
    Settings,
    _canonical_lineage_label,
    _coverage_gap_reason,
    create_app,
    resolve_lineage_term,
)


class DepMapApiTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        root = Path(self.temp.name)
        (root / "depmap-26q1-core").mkdir()
        (root / "depmap-26q1-core" / "lineage_blocks").mkdir()
        (root / "depmap-26q1-full").mkdir()
        tcga_root = root / "depmap-26q1-tcga"
        tcga_project = tcga_root / "projects" / "TCGA-BRCA"
        tcga_project.mkdir(parents=True)
        (tcga_root / "qa.json").write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "release": "26Q1-TCGA-v1",
                    "status": "PASS",
                    "target_gene_count": 18531,
                    "project_count": 1,
                }
            ),
            encoding="utf-8",
        )
        (tcga_root / "project_catalog.csv").write_text(
            "tcga_project,depmap_lineage,status\nTCGA-BRCA,Breast,complete\n",
            encoding="utf-8",
        )
        (tcga_project / "manifest.json").write_text(
            json.dumps({"schema_version": 1, "status": "complete", "tcga_project": "TCGA-BRCA"}),
            encoding="utf-8",
        )
        pq.write_table(
            pa.table(
                {
                    "symbol": ["ESR1", "LOWEVENT"],
                    "hgnc_id": ["HGNC:3467", "HGNC:0"],
                    "entrez_id": [2099, 0],
                    "ensembl_gene_id": ["ENSG00000091831", "ENSG00000000000"],
                    "tcga_project": ["TCGA-BRCA", "TCGA-BRCA"],
                    "depmap_lineage": ["Breast", "Breast"],
                    "expression_available": [True, True],
                    "expression_mapping_basis": ["ensembl_gene_id", "symbol_fallback"],
                    "expression_n": [1090, 20],
                    "expression_median_log2_tpm": [5.25, 0.1],
                    "expression_os_n": [1041, 20],
                    "expression_os_events": [153, 2],
                    "expression_os_score_z": [-2.1, None],
                    "expression_os_p_value": [0.0357, None],
                    "expression_os_fdr": [0.12, None],
                    "expression_dss_n": [990, None],
                    "expression_dss_events": [92, None],
                    "expression_dss_score_z": [-2.4, None],
                    "expression_dss_p_value": [0.0164, None],
                    "expression_dss_fdr": [0.08, None],
                    "expression_dfi_n": [900, None],
                    "expression_dfi_events": [130, None],
                    "expression_dfi_score_z": [-1.5, None],
                    "expression_dfi_p_value": [0.13, None],
                    "expression_dfi_fdr": [0.3, None],
                    "expression_pfi_n": [1000, None],
                    "expression_pfi_events": [210, None],
                    "expression_pfi_score_z": [-2.8, None],
                    "expression_pfi_p_value": [0.0051, None],
                    "expression_pfi_fdr": [0.04, None],
                }
            ),
            tcga_project / "gene_associations.parquet",
        )
        (root / "depmap-26q1-module-catalog.csv").write_text("module\n", encoding="utf-8")
        (root / "depmap-26q1-qa.json").write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "release": "26Q1",
                    "qa_status": "PASS",
                    "module_count": 26,
                }
            ),
            encoding="utf-8",
        )
        self.script = root / "query_depmap_kb.R"
        self.script.write_text("# fixture\n", encoding="utf-8")
        self.settings = Settings(
            knowledge_root=root,
            query_script=self.script,
            api_token="test-token-with-at-least-thirty-two-characters",
        )
        self.queries = []

        async def runner(_settings, query):
            self.queries.append(query)
            return {"mode": query["mode"], "status": "ok"}

        self.client = TestClient(create_app(self.settings, runner))
        self.client.__enter__()

    def tearDown(self):
        self.client.__exit__(None, None, None)
        self.temp.cleanup()

    @property
    def headers(self):
        return {"Authorization": f"Bearer {self.settings.api_token}"}

    def test_health_requires_bearer_and_reports_ready_release(self):
        self.assertEqual(self.client.get("/api/v1/health").status_code, 401)
        response = self.client.get("/api/v1/health", headers=self.headers)
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.json()["status"], "ready")
        self.assertEqual(response.json()["release"], "26Q1")
        self.assertEqual(response.json()["query_contract_version"], 4)
        self.assertIn("lineage_network", response.json()["query_modes"])
        self.assertIn("tcga_expression_survival", response.json()["query_modes"])
        self.assertIn("NOT_RETAINED", response.json()["evidence_statuses"])

    def test_pair_query_is_bounded_and_forwarded(self):
        response = self.client.post(
            "/api/v1/query",
            headers=self.headers,
            json={
                "mode": "pair",
                "module": "effect_correlation",
                "source": "KRAS",
                "target": "RAF1",
            },
        )
        self.assertEqual(response.status_code, 200)
        self.assertEqual(self.queries[0]["source"], "KRAS")

    def test_tcga_query_normalizes_project_and_endpoint_before_forwarding(self):
        response = self.client.post(
            "/api/v1/query",
            headers=self.headers,
            json={
                "mode": "tcga_expression_survival",
                "gene": "esr1",
                "project": "brca",
                "endpoint": "dss",
            },
        )
        self.assertEqual(response.status_code, 200)
        self.assertEqual(self.queries[0]["project"], "TCGA-BRCA")
        self.assertEqual(self.queries[0]["endpoint"], "DSS")

    def test_tcga_query_rejects_unknown_survival_endpoint(self):
        response = self.client.post(
            "/api/v1/query",
            headers=self.headers,
            json={
                "mode": "tcga_expression_survival",
                "gene": "ESR1",
                "endpoint": "RFS",
            },
        )
        self.assertEqual(response.status_code, 422)

    def test_precomputed_tcga_query_returns_bounded_expression_survival_row(self):
        client = TestClient(create_app(self.settings))
        with client:
            response = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "tcga_expression_survival",
                    "gene": "ESR1",
                    "lineage": "Breast cancer",
                    "endpoint": "OS",
                },
            )
        self.assertEqual(response.status_code, 200)
        payload = response.json()
        self.assertEqual(payload["status"], "FOUND")
        self.assertEqual(payload["gene"], "ESR1")
        self.assertEqual(payload["endpoint"], "OS")
        self.assertEqual(payload["rows"][0]["tcga_project"], "TCGA-BRCA")
        self.assertEqual(payload["rows"][0]["expression_mapping_basis"], "ensembl_gene_id")
        self.assertEqual(payload["rows"][0]["expression_os_events"], 153)
        self.assertEqual(payload["manifest"]["multiple_testing"], "BH FDR within TCGA project and survival endpoint")

    def test_precomputed_tcga_query_distinguishes_unmatched_gene(self):
        client = TestClient(create_app(self.settings))
        with client:
            response = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "tcga_expression_survival",
                    "gene": "ABSENT",
                    "project": "TCGA-BRCA",
                },
            )
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.json()["status"], "NOT_COMPUTED")

    def test_precomputed_tcga_query_exposes_survival_ineligibility(self):
        client = TestClient(create_app(self.settings))
        with client:
            response = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "tcga_expression_survival",
                    "gene": "LOWEVENT",
                    "project": "TCGA-BRCA",
                    "endpoint": "OS",
                },
            )
        self.assertEqual(response.status_code, 200)
        payload = response.json()
        self.assertEqual(payload["status"], "INELIGIBLE")
        self.assertEqual(payload["rows"][0]["association_status"], "INELIGIBLE")
        self.assertEqual(payload["rows"][0]["min_events"], 10)

    def test_lineage_alias_is_canonicalized_before_forwarding(self):
        response = self.client.post(
            "/api/v1/query",
            headers=self.headers,
            json={
                "mode": "lineage_network",
                "family": "effect_correlation",
                "lineage": "Breast Cancer",
                "source": "ESR1",
            },
        )
        self.assertEqual(response.status_code, 200)
        self.assertEqual(self.queries[0]["lineage"], "Breast")

    def test_chinese_colon_cancer_maps_to_bowel_for_direction_discovery(self):
        response = self.client.post(
            "/api/v1/query",
            headers=self.headers,
            json={"mode": "lineage_directions", "lineage": "结肠癌", "limit": 20},
        )
        self.assertEqual(response.status_code, 200)
        self.assertEqual(self.queries[0]["lineage"], "Bowel")

    def test_every_canonical_lineage_has_working_chinese_aliases(self):
        self.assertEqual(set(CHINESE_LINEAGE_ALIASES), set(CANONICAL_LINEAGES))
        aliases = [
            alias
            for values in CHINESE_LINEAGE_ALIASES.values()
            for alias in values
        ]
        self.assertEqual(len(aliases), len(set(aliases)))
        for canonical, values in CHINESE_LINEAGE_ALIASES.items():
            for alias in values:
                self.assertEqual(_canonical_lineage_label(alias), canonical)
        self.assertEqual(_canonical_lineage_label("  卵巢 / 输卵管  "), "Ovary Fallopian Tube")

    def test_lineage_resolver_separates_exact_ambiguous_and_model_proposed_terms(self):
        exact = resolve_lineage_term("结直肠癌")
        self.assertEqual(exact["status"], "RESOLVED")
        self.assertEqual(exact["selected_lineage"], "Bowel")
        self.assertFalse(exact["requires_user_confirmation"])

        ambiguous = resolve_lineage_term("白血病")
        self.assertEqual(ambiguous["status"], "AMBIGUOUS")
        self.assertEqual(ambiguous["candidates"], ["Myeloid", "Lymphoid"])
        self.assertTrue(ambiguous["requires_user_confirmation"])

        proposed = resolve_lineage_term(
            "胃食管交界部恶性肿瘤", ["Esophagus Stomach"]
        )
        self.assertEqual(proposed["status"], "PROPOSED")
        self.assertIsNone(proposed["selected_lineage"])
        self.assertEqual(proposed["candidates"], ["Esophagus Stomach"])

        invalid = resolve_lineage_term("一种未知肿瘤", ["Not A Lineage"])
        self.assertEqual(invalid["status"], "INVALID_CANDIDATES")
        self.assertEqual(invalid["invalid_candidates"], ["Not A Lineage"])

        unresolved = resolve_lineage_term("一种未知肿瘤")
        self.assertEqual(unresolved["status"], "UNRESOLVED")
        self.assertEqual(unresolved["candidates"], [])

    def test_coverage_gap_reason_uses_the_scientific_cause_not_r_stack_header(self):
        stderr = "\n".join(
            (
                "Error in read_cell(root, spec, source, target, top = TRUE,  :",
                "  source not found or ineligible: ESR1",
                "Execution halted",
            )
        )
        self.assertEqual(
            _coverage_gap_reason(stderr),
            "source not found or ineligible: ESR1",
        )

    def test_invalid_or_ambiguous_queries_are_rejected(self):
        missing = self.client.post(
            "/api/v1/query",
            headers=self.headers,
            json={"mode": "pair", "module": "effect_correlation", "source": "KRAS"},
        )
        self.assertEqual(missing.status_code, 422)
        extra = self.client.post(
            "/api/v1/query",
            headers=self.headers,
            json={"mode": "core", "gene": "KRAS", "target": "RAF1"},
        )
        self.assertEqual(extra.status_code, 422)
        unsupported = self.client.post(
            "/api/v1/query",
            headers=self.headers,
            json={"mode": "pair", "module": "unknown", "source": "KRAS", "target": "RAF1"},
        )
        self.assertEqual(unsupported.status_code, 422)

    def test_default_core_query_reads_bounded_parquet_rows(self):
        core = self.settings.knowledge_root / "depmap-26q1-core"
        pq.write_table(
            pa.table({"symbol": ["KRAS", "TP53"], "effect_n": [1208, 1208]}),
            core / "gene_core_summary.parquet",
        )
        pq.write_table(
            pa.table({"symbol": ["KRAS"], "lineage": ["Lung"], "effect_n": [91]}),
            core / "lineage_blocks" / "lung.parquet",
        )
        with TestClient(create_app(self.settings)) as client:
            response = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={"mode": "core", "gene": "kras"},
            )
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.json()["gene"], "KRAS")
        self.assertEqual(response.json()["summary"][0]["effect_n"], 1208)
        self.assertEqual(response.json()["lineages"][0]["lineage"], "Lung")

    def _write_manifest(self, path: Path, **values):
        path.mkdir(parents=True, exist_ok=True)
        (path / "manifest.json").write_text(
            json.dumps({"schema_version": 1, **values}), encoding="utf-8"
        )

    def test_lineage_network_returns_found_and_not_retained_without_dense_output(self):
        root = (
            self.settings.knowledge_root
            / "depmap-26q1-full"
            / "lineage_sparse_networks"
            / "effect_correlation"
            / "Lung"
        )
        self._write_manifest(
            root,
            release="26Q1",
            status="complete",
            family="effect_correlation",
            lineage="Lung",
            lineage_sample_n=126,
            top_k_each_direction=100,
        )
        (root / "source_gene_order.csv").write_text(
            "source_index,symbol\n1,KRAS\n", encoding="utf-8"
        )
        (root / "blocks").mkdir()
        pq.write_table(
            pa.table(
                {
                    "source_gene": ["KRAS"],
                    "target_gene": ["RAF1"],
                    "correlation": [0.72],
                    "pair_n": [126],
                    "p_value": [1e-10],
                    "fdr": [1e-7],
                }
            ),
            root / "blocks" / "block_00001_00001.parquet",
        )
        with TestClient(create_app(self.settings)) as client:
            found = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "lineage_network",
                    "family": "effect_correlation",
                    "lineage": "Lung",
                    "source": "KRAS",
                    "target": "RAF1",
                },
            )
            missing = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "lineage_network",
                    "family": "effect_correlation",
                    "lineage": "Lung",
                    "source": "KRAS",
                    "target": "BRAF",
                },
            )
        self.assertEqual(found.status_code, 200)
        self.assertEqual(found.json()["status"], "FOUND")
        self.assertEqual(found.json()["rows"][0]["target_gene"], "RAF1")
        self.assertEqual(found.json()["manifest"]["lineage_sample_n"], 126)
        self.assertEqual(missing.status_code, 200)
        self.assertEqual(missing.json()["status"], "NOT_RETAINED")

    def test_lineage_direction_discovery_selects_precomputed_significant_rows(self):
        root = (
            self.settings.knowledge_root
            / "depmap-26q1-full"
            / "lineage_sparse_networks"
            / "effect_correlation"
            / "Bowel"
        )
        self._write_manifest(
            root,
            release="26Q1",
            status="complete",
            family="effect_correlation",
            lineage="Bowel",
            lineage_sample_n=63,
        )
        pq.write_table(
            pa.table(
                {
                    "family": ["effect_correlation", "effect_correlation"],
                    "lineage": ["Bowel", "Bowel"],
                    "source_gene": ["TSC1", "NOISE1"],
                    "target_gene": ["TSC2", "NOISE2"],
                    "correlation": [0.89, 0.99],
                    "pair_n": [63, 63],
                    "p_value": [1e-12, 0.1],
                    "fdr": [1e-8, 0.8],
                    "direction": ["positive", "positive"],
                    "reverse_correlation": [0.89, 0.99],
                    "reciprocal_rank_max": [1, 1],
                    "reciprocal_score": [0.89, 0.99],
                }
            ),
            root / "reciprocal_pairs.parquet",
        )
        with TestClient(create_app(self.settings)) as client:
            response = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={"mode": "lineage_directions", "lineage": "结肠癌", "limit": 5},
            )
        self.assertEqual(response.status_code, 200)
        payload = response.json()
        self.assertEqual(payload["lineage"], "Bowel")
        effect = next(
            section
            for section in payload["sections"]
            if section["label"] == "effect_correlation"
        )
        self.assertEqual(effect["status"], "FOUND")
        self.assertEqual(effect["returned_candidate_count"], 1)
        self.assertEqual(effect["rows"][0]["source_gene"], "TSC1")
        self.assertEqual(payload["selection_policy"]["network_and_enrichment_fdr_max"], 0.05)
        self.assertEqual(payload["topic_candidates"][0]["topic_type"], "reciprocal_dependency_pair")
        self.assertEqual(payload["topic_candidates"][0]["anchors"]["target_gene"], "TSC2")
        self.assertFalse(payload["new_analysis_started"])

    def test_lineage_statuses_distinguish_ineligible_not_computed_and_unavailable(self):
        cnv = (
            self.settings.knowledge_root
            / "depmap-26q1-full"
            / "lineage_cnv_amplification_dependency"
            / "Adrenal_Gland"
        )
        self._write_manifest(
            cnv,
            status="ineligible_sample_n",
            lineage="Adrenal Gland",
            lineage_sample_n=1,
            min_amp=5,
            min_wt=10,
        )
        enrichment = (
            self.settings.knowledge_root
            / "depmap-26q1-full"
            / "lineage_gene_enrichment"
        )
        enrichment.mkdir()
        with TestClient(create_app(self.settings)) as client:
            ineligible = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "lineage_cnv",
                    "lineage": "Adrenal Gland",
                    "source": "MYC",
                    "target": "CDK1",
                },
            )
            not_computed = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "enrichment",
                    "lineage": "Unknown lineage",
                    "source": "KRAS",
                },
            )
            unavailable = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "lineage_network",
                    "family": "expression_dependency",
                    "lineage": "Lung",
                    "source": "KRAS",
                },
            )
        self.assertEqual(ineligible.json()["status"], "INELIGIBLE")
        self.assertEqual(ineligible.json()["manifest"]["min_amp"], 5)
        self.assertEqual(not_computed.json()["status"], "NOT_COMPUTED")
        self.assertEqual(unavailable.json()["status"], "MODULE_UNAVAILABLE")

    def test_lineage_catalog_inventory_needs_no_invented_anchor_gene(self):
        network = (
            self.settings.knowledge_root
            / "depmap-26q1-full"
            / "lineage_sparse_networks"
            / "effect_correlation"
            / "Bowel"
        )
        self._write_manifest(
            network,
            status="complete",
            lineage="Bowel",
            lineage_sample_n=63,
            family="effect_correlation",
        )
        with TestClient(create_app(self.settings)) as client:
            response = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={"mode": "lineage_catalog", "lineage": "Colorectal"},
            )
        self.assertEqual(response.status_code, 200)
        result = response.json()
        self.assertEqual(result["lineage"], "Bowel")
        self.assertEqual(result["scope"], "lineage_availability_only")
        self.assertEqual(result["summary"]["available_module_count"], 1)
        effect = next(
            item for item in result["modules"]
            if item["label"] == "network:effect_correlation"
        )
        self.assertEqual(effect["status"], "FOUND")
        self.assertEqual(effect["manifest"]["lineage_sample_n"], 63)

    def test_lineage_drug_requires_a_drug_or_gene(self):
        with TestClient(create_app(self.settings)) as client:
            response = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "lineage_drug",
                    "omic": "effect",
                    "lineage": "Lung",
                },
            )
        self.assertEqual(response.status_code, 422)

    def test_lineage_drug_returns_named_bounded_rows(self):
        root = (
            self.settings.knowledge_root
            / "depmap-26q1-full"
            / "lineage_prism_associations"
            / "effect"
            / "Lung"
        )
        self._write_manifest(
            root,
            status="complete",
            lineage="Lung",
            feature="effect",
            matched_sample_count=58,
            min_n=10,
        )
        pq.write_table(
            pa.table(
                {
                    "CompoundName": ["Example inhibitor"],
                    "CompoundID": ["DPC-000001"],
                }
            ),
            root / "drug_metadata.parquet",
        )
        pq.write_table(
            pa.table(
                {
                    "lineage": ["Lung"],
                    "feature": ["effect"],
                    "drug_id": ["DPC-000001"],
                    "gene": ["KRAS"],
                    "n": [58],
                    "pearson_r": [0.61],
                    "p_value": [1e-7],
                    "fdr_within_drug": [0.004],
                    "retained_by": ["drug_top+gene_top"],
                }
            ),
            root / "associations.parquet",
        )
        with TestClient(create_app(self.settings)) as client:
            response = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "lineage_drug",
                    "omic": "effect",
                    "lineage": "Lung",
                    "target": "KRAS",
                    "limit": 1,
                },
            )
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.json()["status"], "FOUND")
        self.assertEqual(response.json()["rows"][0]["drug_name"], "Example inhibitor")
        self.assertEqual(response.json()["rows"][0]["n"], 58)


if __name__ == "__main__":
    unittest.main()
