import json
import gzip
import os
import sqlite3
import tempfile
import unittest
from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq
from fastapi.testclient import TestClient

from services.depmap_api.app import (
    CANONICAL_LINEAGES,
    CHINESE_LINEAGE_ALIASES,
    QUERY_FIELD_ORDER,
    Settings,
    QueryRequest,
    _canonical_lineage_label,
    _coerce_csv_value,
    _coverage_gap_reason,
    _r_query_command,
    create_app,
    resolve_lineage_term,
)


class QueryContractTests(unittest.TestCase):
    def test_scientific_csv_values_keep_small_and_threshold_precision(self):
        self.assertEqual(_coerce_csv_value("1.6156e-8"), 1.6156e-8)
        self.assertGreater(_coerce_csv_value("3.2669e-12"), 0.0)
        self.assertEqual(_coerce_csv_value("0.049999999"), 0.049999999)
        self.assertEqual(_coerce_csv_value("0.050000001"), 0.050000001)

    def test_common_essential_options_are_forwarded_to_r_runner(self):
        self.assertIn("exclude_common_essential", QUERY_FIELD_ORDER)
        self.assertIn("common_essential_source", QUERY_FIELD_ORDER)
        settings = Settings(
            knowledge_root=Path("knowledge"),
            query_script=Path("query.R"),
            api_token="test-token-with-at-least-thirty-two-characters",
        )
        command = _r_query_command(
            settings,
            {
                "mode": "pan_cancer_dependency",
                "exclude_common_essential": True,
                "common_essential_source": "depmap_26q1",
            },
        )
        self.assertIn("--exclude-common-essential", command)
        self.assertEqual(command[command.index("--exclude-common-essential") + 1], "True")
        self.assertIn("--common-essential-source", command)

    def test_mutation_anchor_has_explicit_lineage_event_and_tier_contract(self):
        request = QueryRequest(
            mode="mutation_anchor", lineage="Lung", event="damaging",
            anchor_tier="priority", include_common_essential=False, limit=20,
        )
        self.assertEqual(request.bounded_dict()["lineage"], "Lung")
        with self.assertRaises(ValueError):
            QueryRequest(mode="mutation_anchor", event="damaging")
        exact = QueryRequest(
            mode="mutation_anchor", lineage="肝癌", gene="ptk7", event="damaging",
        )
        self.assertEqual(exact.bounded_dict()["lineage"], "Liver")
        self.assertEqual(exact.bounded_dict()["gene"], "ptk7")
        lineage_dep = QueryRequest(
            mode="lineage_mutation_dependency",
            lineage="Liver",
            source="PTK7",
            event="damaging_mutation",
            limit=5,
        )
        self.assertEqual(lineage_dep.bounded_dict()["source"], "PTK7")
        with self.assertRaises(ValueError):
            QueryRequest(mode="lineage_mutation_dependency", lineage="Liver")

    def test_tf_dependency_accepts_exact_pair_or_bounded_top_query(self):
        exact = QueryRequest(mode="tf_dependency", source="STAT3", target="GPX4", limit=20)
        self.assertEqual(exact.bounded_dict()["target"], "GPX4")
        top = QueryRequest(mode="tf_dependency", source="STAT3", limit=5)
        self.assertNotIn("target", top.bounded_dict())

    def test_tf_dependency_accepts_universe_and_bulk_ranking(self):
        universe = QueryRequest(mode="tf_dependency", view="universe", limit=20)
        self.assertEqual(universe.bounded_dict()["view"], "universe")
        bulk = QueryRequest(mode="tf_dependency", limit=5)
        self.assertNotIn("source", bulk.bounded_dict())

    def test_tf_dependency_requires_tf_source(self):
        with self.assertRaises(ValueError):
            QueryRequest(mode="tf_dependency", target="GPX4")

    def test_biomarker_target_requires_dependency_target(self):
        request = QueryRequest(mode="biomarker_target", target="GPX4")
        self.assertEqual(request.bounded_dict()["target"], "GPX4")
        with self.assertRaises(ValueError):
            QueryRequest(mode="biomarker_target")


class DepMapApiTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        root = Path(self.temp.name)
        (root / "depmap-26q1-core").mkdir()
        (root / "depmap-26q1-core" / "lineage_blocks").mkdir()
        (root / "depmap-26q1-full").mkdir()
        subtype_root = root / "depmap-26q1-full" / "subtype_dependency"
        subtype_unit = subtype_root / "FEATURE__BOWEL__MSI"
        subtype_unit.mkdir(parents=True)
        (subtype_root / "manifest.json").write_text(
            json.dumps({"status": "complete", "eligible_contrast_count": 1, "target_gene_count": 2}),
            encoding="utf-8",
        )
        (subtype_root / "contrast_catalog.csv").write_text(
            "contrast_id,contrast_kind,lineage,subtype_label,group_n,control_n,eligible,definition,ineligibility_reason\n"
            "FEATURE__BOWEL__MSI,curated_model_feature,Bowel,MSI,10,53,TRUE,MSI within Bowel,\n",
            encoding="utf-8",
        )
        (subtype_unit / "manifest.json").write_text(
            json.dumps({"status": "complete", "selective_fdr_hit_count": 1}),
            encoding="utf-8",
        )
        subtype_header = (
            "gene,contrast_id,contrast_kind,lineage,subtype_label,group_n,control_n,"
            "subtype_mean_gene_effect,control_mean_gene_effect,effect_size,moderated_t,"
            "p_value,fdr,stronger_subtype_dependency,passes_fdr,rank_stronger_dependency\n"
        )
        with gzip.open(subtype_unit / "all_genes.csv.gz", "wt", encoding="utf-8") as handle:
            handle.write(subtype_header)
            handle.write("WRN,FEATURE__BOWEL__MSI,curated_model_feature,Bowel,MSI,10,53,-1.8,-0.2,-1.6,-8,1e-11,4e-7,TRUE,TRUE,1\n")
            handle.write("TP53,FEATURE__BOWEL__MSI,curated_model_feature,Bowel,MSI,10,53,-0.1,-0.1,0,0,1,1,FALSE,FALSE,100\n")
        with gzip.open(subtype_unit / "selective_hits.csv.gz", "wt", encoding="utf-8") as handle:
            handle.write(subtype_header)
            handle.write("WRN,FEATURE__BOWEL__MSI,curated_model_feature,Bowel,MSI,10,53,-1.8,-0.2,-1.6,-8,1e-11,4e-7,TRUE,TRUE,1\n")

        coamp_root = root / "depmap-26q1-full" / "coamplification_dependency"
        exhaustive_root = coamp_root / "exhaustive_high_confidence"
        adjusted_root = coamp_root / "lineage_adjusted"
        exhaustive_root.mkdir(parents=True)
        adjusted_root.mkdir(parents=True)
        for path, payload in (
            (exhaustive_root / "manifest.json", {"status": "complete", "input_directional_pair_count": 1}),
            (adjusted_root / "manifest.json", {"status": "complete", "input_directional_pair_count": 1}),
        ):
            path.write_text(json.dumps(payload), encoding="utf-8")
        with gzip.open(exhaustive_root / "screen_pair_catalog.csv.gz", "wt", encoding="utf-8") as handle:
            handle.write("screen_pair_id,pair_id,source_gene,partner_gene,source_amp_n,partner_amp_n,coamplified_n,source_only_n,partner_only_n,jaccard\n")
            handle.write("COAMP-HC-000001,COAMP-1,CTTN,RNF121,70,60,28,42,32,0.27\n")
        with gzip.open(exhaustive_root / "significant_hits.csv.gz", "wt", encoding="utf-8") as handle:
            handle.write("screen_pair_id,source_gene,partner_gene,target_gene,coamplified_n,source_only_n,mean_difference,fdr_coamplified_more_dependent,rank_within_pair\n")
            handle.write("COAMP-HC-000001,CTTN,RNF121,TFEC,28,42,-0.12,0.013,1\n")
        with gzip.open(adjusted_root / "significant_hits.csv.gz", "wt", encoding="utf-8") as handle:
            handle.write("screen_pair_id,source_gene,partner_gene,target_gene,lineage_adjusted_effect,fdr_within_pair,rank_within_pair\n")
            handle.write("COAMP-HC-000001,CTTN,RNF121,TFEC,-0.27,0.0005,1\n")
        with gzip.open(adjusted_root / "pair_lineage_audit.csv.gz", "wt", encoding="utf-8") as handle:
            handle.write("screen_pair_id,source_gene,partner_gene,model_n,coamplified_n,source_only_n,informative_lineage_count,estimable\n")
            handle.write("COAMP-HC-000001,CTTN,RNF121,53,19,34,9,TRUE\n")

        true_love_root = root / "depmap-26q1-full" / "true_love_gene"
        stable_root = true_love_root / "high_confidence_stability"
        stable_root.mkdir(parents=True)
        (true_love_root / "manifest.json").write_text(json.dumps({"status": "complete"}), encoding="utf-8")
        (stable_root / "manifest.json").write_text(json.dumps({"status": "complete", "pair_count": 1}), encoding="utf-8")
        with gzip.open(stable_root / "final_high_confidence_true_love_genes.csv.gz", "wt", encoding="utf-8") as handle:
            handle.write("true_love_pair_id,gene_a,gene_b,worst_direction_fdr,strongest_absolute_correlation,bootstrap_reciprocal_stability\n")
            handle.write("TL-1,KRAS,NRAS,0.001,0.72,0.94\n")
        derived_root = true_love_root / "tm00_derived_catalogs_26Q1"
        derived_root.mkdir()
        (derived_root / "manifest.json").write_text(
            json.dumps({"status": "complete", "quality_min_pair_n": 500}), encoding="utf-8"
        )
        with gzip.open(derived_root / "negative_codependency_r_lt_minus_0.3_n500.csv.gz", "wt", encoding="utf-8") as handle:
            handle.write("gene_a,gene_b,correlation,pair_n,p_value\nKRAS,NRAS,-0.41,1208,1e-30\n")
        with gzip.open(derived_root / "negative_codependency_r_lt_minus_0.3_legacy.csv.gz", "wt", encoding="utf-8") as handle:
            handle.write("gene_a,gene_b,correlation,pair_n,p_value,coverage_pass_n500\nKRAS,NRAS,-0.41,1208,1e-30,TRUE\nLOWA,LOWB,-0.99,4,0.01,FALSE\n")
        with gzip.open(derived_root / "positive_reciprocal_top20_n500.csv.gz", "wt", encoding="utf-8") as handle:
            handle.write("gene_a,gene_b,correlation_a_to_b,correlation_b_to_a,rank_a_to_b,rank_b_to_a,reciprocal_rank_sum\nKRAS,RAF1,0.66,0.66,2,3,5\n")

        synthetic_root = root / "depmap-26q1-full" / "observational_synthetic_lethal_candidates"
        synthetic_root.mkdir(parents=True)
        (synthetic_root / "manifest.json").write_text(json.dumps({"status": "complete"}), encoding="utf-8")
        with gzip.open(synthetic_root / "pair_evidence_summary.csv.gz", "wt", encoding="utf-8") as handle:
            handle.write("source_gene,target_gene,evidence_family_count,evidence_families,best_fdr,strongest_mean_difference,max_event_n,max_control_n\n")
            handle.write("ARID1A,ARID1B,2,damaging_mutation;cnv_amplification,0.002,-0.45,22,80\n")

        three_d_root = root / "depmap-26q1-3d" / "dependency_profiles"
        three_d_unit = three_d_root / "three_d_all"
        three_d_unit.mkdir(parents=True)
        (three_d_root / "manifest.json").write_text(json.dumps({"status": "complete"}), encoding="utf-8")
        (three_d_root / "group_catalog.csv").write_text(
            "group,screen_types,screen_count,robust_dependency_count,status\nthree_d_all,3DO+3DN,108,1,complete\n",
            encoding="utf-8",
        )
        (three_d_unit / "manifest.json").write_text(json.dumps({"status": "complete"}), encoding="utf-8")
        with gzip.open(three_d_unit / "all_genes.csv.gz", "wt", encoding="utf-8") as handle:
            handle.write("gene,valid_gene_effect_n,mean_gene_effect,robust_group_dependency\n")
            handle.write("KRAS,108,-0.62,TRUE\n")
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
                    "expression_mapping_basis": ["gene_symbol", "ensembl_fallback"],
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
        self.assertEqual(response.json()["query_contract_version"], 11)
        self.assertIn("lineage_mutation_dependency", response.json()["query_modes"])
        self.assertIn("NOT_OBSERVED", response.json()["evidence_statuses"])
        self.assertIn("COVERAGE_GAP", response.json()["evidence_statuses"])
        self.assertIn("lineage_network", response.json()["query_modes"])
        self.assertIn("tcga_expression_survival", response.json()["query_modes"])
        self.assertIn("subtype", response.json()["query_modes"])
        self.assertIn("coamplification", response.json()["query_modes"])
        self.assertIn("true_love", response.json()["query_modes"])
        self.assertIn("synthetic_lethal", response.json()["query_modes"])
        self.assertIn("three_d", response.json()["query_modes"])
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

    def test_precomputed_subtype_query_supports_catalog_gene_and_ranking(self):
        client = TestClient(create_app(self.settings))
        with client:
            catalog = client.post(
                "/api/v1/query", headers=self.headers,
                json={"mode": "subtype", "lineage": "结肠癌", "limit": 10},
            ).json()
            gene = client.post(
                "/api/v1/query", headers=self.headers,
                json={"mode": "subtype", "lineage": "Bowel", "gene": "wrn"},
            ).json()
            ranked = client.post(
                "/api/v1/query", headers=self.headers,
                json={"mode": "subtype", "contrast": "FEATURE__BOWEL__MSI", "limit": 10},
            ).json()
        self.assertEqual(catalog["status"], "FOUND")
        self.assertEqual(catalog["rows"][0]["contrast_id"], "FEATURE__BOWEL__MSI")
        self.assertEqual(gene["status"], "FOUND")
        self.assertEqual(gene["rows"][0]["gene"], "WRN")
        self.assertEqual(ranked["rows"][0]["effect_size"], -1.6)

    def test_precomputed_coamplification_query_distinguishes_retained_state(self):
        client = TestClient(create_app(self.settings))
        with client:
            found = client.post(
                "/api/v1/query", headers=self.headers,
                json={"mode": "coamplification", "source": "cttn", "partner": "rnf121", "target": "tfec"},
            ).json()
            not_retained = client.post(
                "/api/v1/query", headers=self.headers,
                json={"mode": "coamplification", "source": "CTTN", "partner": "RNF121", "target": "TP53"},
            ).json()
        self.assertEqual(found["status"], "FOUND")
        self.assertEqual(found["hits"][0]["lineage_adjusted_effect"], -0.27)
        self.assertEqual(not_retained["status"], "NOT_RETAINED")

    def test_provider_schema_rejects_catalog_conditional_args_with_envelope(self):
        client = TestClient(create_app(self.settings))
        with client:
            coverage = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "true_love",
                    "catalog": "stable_negative_rank1",
                    "coverage": "quality",
                    "limit": 20,
                },
            )
            oversize = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={"mode": "lineage_directions", "lineage": "Liver", "limit": 60},
            )
            true_love_limit = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={"mode": "true_love", "catalog": "stable_negative_rank1", "limit": 150},
            )
            valid_derived = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "true_love",
                    "catalog": "negative_r_lt_minus_0_3",
                    "coverage": "quality",
                    "gene": "KRAS",
                    "limit": 5,
                },
            )
        self.assertEqual(coverage.status_code, 422)
        self.assertTrue(coverage.json()["schema_error"])
        self.assertEqual(coverage.json()["status"], "INELIGIBLE")
        self.assertNotIn("Traceback", coverage.text)
        self.assertEqual(oversize.status_code, 422)
        self.assertTrue(oversize.json()["schema_error"])
        self.assertEqual(true_love_limit.status_code, 422)
        self.assertTrue(true_love_limit.json()["schema_error"])
        self.assertEqual(valid_derived.status_code, 200)
        self.assertEqual(valid_derived.json()["status"], "FOUND")

    def test_true_love_scope_is_lineage_or_pancancer_not_a_filter(self):
        client = TestClient(create_app(self.settings))
        with client:
            pancancer = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={"mode": "true_love", "catalog": "stable_negative_rank1", "limit": 5},
            ).json()
            missing_lineage_table = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "true_love",
                    "scope": "lineage",
                    "lineage": "Liver",
                    "limit": 5,
                },
            ).json()
        self.assertEqual(pancancer["status"], "FOUND")
        self.assertEqual(pancancer["scope"], "pancancer")
        self.assertEqual(
            pancancer["pair_definition"],
            "stable_mutual_rank1_negative_codependency",
        )
        self.assertEqual(missing_lineage_table["status"], "COVERAGE_GAP")
        self.assertEqual(missing_lineage_table["scope"], "lineage")
        self.assertEqual(missing_lineage_table["lineage"], "Liver")
        self.assertNotEqual(missing_lineage_table["status"], "FOUND")

        lineage_root = (
            self.settings.knowledge_root
            / "depmap-26q1-full"
            / "true_love_gene"
            / "lineage_scope"
            / "Liver"
        )
        lineage_root.mkdir(parents=True)
        (lineage_root / "manifest.json").write_text(
            json.dumps({"status": "complete"}), encoding="utf-8"
        )
        with gzip.open(lineage_root / "pairs.csv.gz", "wt", encoding="utf-8") as handle:
            handle.write(
                "true_love_pair_id,gene_a,gene_b,worst_direction_fdr,"
                "strongest_absolute_correlation,bootstrap_reciprocal_stability\n"
            )
            handle.write("LIV-1,FOXA1,HNF4A,0.01,0.55,0.81\n")
        with client:
            lineage_hit = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "true_love",
                    "scope": "lineage",
                    "lineage": "肝癌",
                    "limit": 5,
                },
            ).json()
            mixed = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "true_love",
                    "scope": "pancancer",
                    "lineage": "Liver",
                    "limit": 5,
                },
            )
        self.assertEqual(lineage_hit["status"], "FOUND")
        self.assertEqual(lineage_hit["rows"][0]["gene_a"], "FOXA1")
        self.assertNotEqual(lineage_hit["rows"][0]["gene_a"], "KRAS")
        self.assertEqual(mixed.status_code, 422)
        self.assertTrue(mixed.json()["schema_error"])

    def test_true_love_synthetic_lethal_and_three_d_are_bounded_precomputed_queries(self):
        client = TestClient(create_app(self.settings))
        with client:
            true_love = client.post(
                "/api/v1/query", headers=self.headers,
                json={"mode": "true_love", "gene": "kras", "partner": "nras", "limit": 5},
            ).json()
            synthetic = client.post(
                "/api/v1/query", headers=self.headers,
                json={"mode": "synthetic_lethal", "source": "arid1a", "target": "arid1b", "limit": 5},
            ).json()
            synthetic_forward = client.post(
                "/api/v1/query", headers=self.headers,
                json={"mode": "synthetic_lethal", "source": "arid1a", "limit": 5},
            ).json()
            synthetic_reverse = client.post(
                "/api/v1/query", headers=self.headers,
                json={"mode": "synthetic_lethal", "target": "arid1b", "limit": 5},
            ).json()
            three_d = client.post(
                "/api/v1/query", headers=self.headers,
                json={"mode": "three_d", "family": "dependency_profiles", "cohort": "three_d_all", "gene": "kras", "limit": 5},
            ).json()
        self.assertEqual(true_love["status"], "FOUND")
        self.assertEqual(true_love["rows"][0]["bootstrap_reciprocal_stability"], 0.94)
        with client:
            negative = client.post(
                "/api/v1/query", headers=self.headers,
                json={"mode": "true_love", "catalog": "negative_r_lt_minus_0_3", "coverage": "quality", "gene": "KRAS", "limit": 5},
            ).json()
            positive = client.post(
                "/api/v1/query", headers=self.headers,
                json={"mode": "true_love", "catalog": "positive_reciprocal_top20", "coverage": "quality", "gene": "KRAS", "limit": 5},
            ).json()
            all_rows = client.post(
                "/api/v1/query", headers=self.headers,
                json={"mode": "true_love", "catalog": "negative_r_lt_minus_0_3", "limit": 5},
            ).json()
        self.assertEqual(negative["rows"][0]["correlation"], -0.41)
        self.assertEqual(positive["rows"][0]["gene_b"], "RAF1")
        self.assertEqual(all_rows["coverage"], "all")
        self.assertEqual(all_rows["summary"]["matched_pair_count"], 2)
        self.assertEqual(synthetic["status"], "FOUND")
        self.assertEqual(synthetic["rows"][0]["target_gene"], "ARID1B")
        self.assertEqual(synthetic_forward["status"], "FOUND")
        self.assertEqual(synthetic_forward["rows"][0]["source_gene"], "ARID1A")
        self.assertEqual(synthetic_reverse["status"], "FOUND")
        self.assertEqual(synthetic_reverse["rows"][0]["target_gene"], "ARID1B")
        self.assertEqual(three_d["status"], "FOUND")
        self.assertEqual(three_d["rows"][0]["mean_gene_effect"], -0.62)

    def test_true_love_prefers_bounded_sqlite_index(self):
        index = Path(self.temp.name) / "depmap-26q1-query-index.sqlite"
        db = sqlite3.connect(index)
        try:
            db.execute(
                "CREATE TABLE true_love (catalog TEXT, coverage TEXT, gene_a TEXT, gene_b TEXT, sort_1 REAL, sort_2 REAL, row_json TEXT)"
            )
            indexed = {
                "gene_a": "KRAS", "gene_b": "NRAS",
                "bootstrap_reciprocal_stability": "0.99",
                "worst_direction_fdr": "0.0001",
            }
            db.execute(
                "INSERT INTO true_love VALUES (?,?,?,?,?,?,?)",
                ("stable_negative_rank1", "all", "KRAS", "NRAS", -0.99, 0.0001, json.dumps(indexed)),
            )
            db.commit()
        finally:
            db.close()
        client = TestClient(create_app(self.settings))
        with client:
            result = client.post(
                "/api/v1/query", headers=self.headers,
                json={"mode": "true_love", "gene": "KRAS", "partner": "NRAS", "limit": 5},
            ).json()
        self.assertEqual(result["status"], "FOUND")
        self.assertEqual(result["rows"][0]["bootstrap_reciprocal_stability"], 0.99)
        self.assertTrue(any(path.endswith("depmap-26q1-query-index.sqlite") for path in result["provenance"]))

    def test_biomarker_intent_reads_indexed_target_and_cache_state(self):
        index = Path(self.temp.name) / "depmap-26q1-query-index.sqlite"
        db = sqlite3.connect(index)
        try:
            db.execute("CREATE TABLE biomarker_target (target_gene TEXT PRIMARY KEY, eligible INTEGER, row_json TEXT)")
            row = {"target_gene": "GPX4", "sample_n": "1208", "sd_gene_effect": "0.21", "eligible_for_nested_model": "TRUE"}
            db.execute("INSERT INTO biomarker_target VALUES (?,?,?)", ("GPX4", 1, json.dumps(row)))
            db.commit()
        finally:
            db.close()
        client = TestClient(create_app(self.settings))
        with client:
            result = client.post(
                "/api/v1/query", headers=self.headers,
                json={"mode": "biomarker_target", "target": "gpx4"},
            ).json()
        self.assertEqual(result["status"], "FOUND")
        self.assertEqual(result["eligibility"]["target_gene"], "GPX4")
        self.assertEqual(result["eligibility"]["sample_n"], 1208)
        self.assertFalse(result["cached_model"])

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
        self.assertEqual(payload["rows"][0]["expression_mapping_basis"], "gene_symbol")
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

    def test_cancer_dependency_ranking_is_canonical_and_bounded(self):
        response = self.client.post(
            "/api/v1/query",
            headers=self.headers,
            json={
                "mode": "lineage_dependency",
                "lineage": "乳腺癌",
                "ranking": "selective",
                "exclude_common_essential": True,
                "common_essential_source": "depmap_26q1",
                "limit": 10,
            },
        )
        self.assertEqual(response.status_code, 200)
        self.assertEqual(
            self.queries[0],
            {
                "mode": "lineage_dependency",
                "lineage": "Breast",
                "ranking": "selective",
                "exclude_common_essential": True,
                "common_essential_source": "depmap_26q1",
                "limit": 10,
            },
        )

        invalid = self.client.post(
            "/api/v1/query",
            headers=self.headers,
            json={
                "mode": "lineage_dependency",
                "lineage": "Breast",
                "ranking": "logfc",
                "limit": 10,
            },
        )
        self.assertEqual(invalid.status_code, 422)

    def test_pan_cancer_dependency_summary_is_bounded(self):
        response = self.client.post(
            "/api/v1/query",
            headers=self.headers,
            json={
                "mode": "pan_cancer_dependency",
                "ranking": "selective",
                "exclude_common_essential": True,
                "common_essential_source": "depmap_26q1",
                "limit": 5,
            },
        )
        self.assertEqual(response.status_code, 200)
        self.assertEqual(self.queries[-1]["mode"], "pan_cancer_dependency")
        self.assertEqual(self.queries[-1]["limit"], 5)

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
            min_n=10,
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

    def _write_direction_network(self, family, min_n, cohort_n=25, status="complete"):
        root = (
            self.settings.knowledge_root / "depmap-26q1-full"
            / "lineage_sparse_networks" / family / "Liver"
        )
        manifest = dict(status=status, lineage="Liver", lineage_sample_n=cohort_n)
        if min_n is not None:
            manifest["min_n"] = min_n
        self._write_manifest(root, **manifest)
        boundary = min_n if type(min_n) is int and min_n > 0 else 10
        rows = {
            "family": [family] * 4,
            "lineage": ["Liver"] * 4,
            "source_gene": ["TOO_SMALL", "BOUNDARY", "SUPPORTED", "NOISE"],
            "target_gene": ["TARGET"] * 4,
            "correlation": [0.99, 0.85, 0.75, 0.95],
            "pair_n": [boundary - 1, boundary, cohort_n, cohort_n],
            "p_value": [1e-8, 1e-8, 1e-8, 0.1],
            "fdr": [1e-6, 1e-6, 1e-6, 0.8],
        }
        if family == "expression_dependency":
            rows["rank_absolute"] = [1, 2, 3, 4]
            path = root / "blocks" / "block_00001_00004.parquet"
            path.parent.mkdir()
        else:
            rows.update(
                direction=["positive"] * 4,
                reverse_correlation=rows["correlation"],
                reciprocal_rank_max=[1, 2, 3, 4],
                reciprocal_score=rows["correlation"],
            )
            path = root / "reciprocal_pairs.parquet"
        pq.write_table(pa.table(rows), path)

    def _direction_payload(self):
        with TestClient(create_app(self.settings)) as client:
            response = client.post(
                "/api/v1/query", headers=self.headers,
                json={"mode": "lineage_directions", "lineage": "Liver", "limit": 5},
            )
        self.assertEqual(response.status_code, 200)
        return response.json()

    def test_directions_use_each_network_manifest_minimum_for_small_cohorts(self):
        minima = {"effect_correlation": 10, "expression_correlation": 12, "expression_dependency": 15}
        for family, min_n in minima.items():
            self._write_direction_network(family, min_n)
        payload = self._direction_payload()
        self.assertEqual(payload["selection_policy"]["network_pair_n_min_by_family"], minima)
        self.assertEqual(payload["selection_policy"]["network_pair_n_min"], 10)
        for section in payload["sections"]:
            if section["label"] not in minima:
                continue
            self.assertEqual(section["status"], "FOUND")
            self.assertEqual(section["selection_filters"]["pair_n_min"], minima[section["label"]])
            self.assertEqual(section["selection_filters"]["pair_n_min_source"], "manifest.min_n")
            self.assertEqual(section["eligible_retained_row_count"], 2)
            self.assertEqual([r["source_gene"] for r in section["rows"]], ["BOUNDARY", "SUPPORTED"])
        self.assertFalse(payload["new_analysis_started"])

    def test_directions_preserve_a_stricter_manifest_minimum(self):
        self._write_direction_network("effect_correlation", 35, cohort_n=40)
        section = self._direction_payload()["sections"][0]
        self.assertEqual(section["selection_filters"]["pair_n_min"], 35)
        self.assertEqual([r["pair_n"] for r in section["rows"]], [35, 40])

    def test_directions_report_invalid_minimum_without_blocking_other_networks(self):
        self._write_direction_network("expression_dependency", 10)
        for min_n in (None, 0, -1, True, "10", 10.5):
            with self.subTest(min_n=min_n):
                self._write_direction_network("effect_correlation", min_n)
                payload = self._direction_payload()
                section = payload["sections"][0]
                self.assertEqual(section["status"], "MODULE_UNAVAILABLE")
                self.assertIn("manifest.min_n", section["reason"])
                self.assertEqual(section["rows"], [])
                self.assertNotIn("effect_correlation", payload["selection_policy"]["network_pair_n_min_by_family"])
                self.assertEqual(payload["sections"][2]["status"], "FOUND")

    def test_directions_report_cohort_below_manifest_minimum_as_ineligible(self):
        self._write_direction_network("effect_correlation", 30, cohort_n=25)
        section = self._direction_payload()["sections"][0]
        self.assertEqual(section["status"], "INELIGIBLE")
        self.assertIn("lineage_sample_n", section["reason"])
        self.assertEqual(section["rows"], [])

    def test_directions_do_not_query_incomplete_expression_dependency_modules(self):
        self._write_direction_network("expression_dependency", 10, status="building")
        section = self._direction_payload()["sections"][2]
        self.assertEqual(section["status"], "NOT_COMPUTED")
        self.assertEqual(section["rows"], [])

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
        self.assertEqual(result["summary"]["available_module_count"], 2)
        effect = next(
            item for item in result["modules"]
            if item["label"] == "network:effect_correlation"
        )
        self.assertEqual(effect["status"], "FOUND")
        self.assertEqual(effect["manifest"]["lineage_sample_n"], 63)
        subtype = next(
            item for item in result["modules"]
            if item["label"] == "subtype:dependency"
        )
        self.assertEqual(subtype["status"], "FOUND")
        self.assertEqual(subtype["contrasts"], ["FEATURE__BOWEL__MSI"])

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


def write_lineage_mutation_fixtures(root: Path) -> None:
    module = (
        root / "analysis-modules" / "癌种内突变锚定基因选择" / "cancer_anchor_catalog_v2"
    )
    liver = module / "by_cancer" / "Liver"
    (liver / "data").mkdir(parents=True)
    (liver / "results").mkdir(parents=True)
    official = liver / "dependency_analysis" / "depmap_official_gene_effect_v2"
    official.mkdir(parents=True)
    (module / "by_cancer" / "Lung").mkdir(parents=True)
    (module / "manifest.json").write_text(
        json.dumps({"status": "complete", "lineage_count": 2}), encoding="utf-8"
    )
    (liver / "manifest.json").write_text(
        json.dumps(
            {
                "status": "complete",
                "lineage": "Liver",
                "cohort_n": 25,
                "thresholds": {
                    "standard": {"min_mut": 3, "min_wt": 5},
                    "strict": {"min_mut": 10, "min_wt": 5},
                },
                "event_definitions": ["AnySelected", "Damaging", "Hotspot"],
            }
        ),
        encoding="utf-8",
    )
    (module / "by_cancer" / "Lung" / "manifest.json").write_text(
        json.dumps({"status": "complete", "lineage": "Lung", "cohort_n": 80}),
        encoding="utf-8",
    )
    decoys = "".join(
        f"DECOY{index:02d},Liver,20,20,Damaging,0.5,TRUE,TRUE,FALSE\n"
        for index in range(30)
    )
    (liver / "data" / "gene_by_lineage_mutation_menu.csv").write_text(
        "gene,lineage,mut_n,wt_n,matrix,mut_rate,pass_standard,pass_strict,is_common_essential\n"
        + decoys
        + "PTK7,Liver,1,24,Damaging,0.04,FALSE,FALSE,FALSE\n"
        + "PTK7,Liver,2,23,AnySelected,0.08,FALSE,FALSE,FALSE\n"
        + "TP53,Liver,18,7,Damaging,0.72,TRUE,TRUE,FALSE\n"
        + "TERT,Liver,16,9,Hotspot,0.64,TRUE,TRUE,FALSE\n"
        + "AXIN1,Liver,5,20,Damaging,0.2,TRUE,FALSE,FALSE\n"
        + "WTFAIL,Liver,12,2,Damaging,0.86,FALSE,FALSE,FALSE\n"
        + "ROLEOG,Liver,12,20,Damaging,0.38,TRUE,TRUE,FALSE\n",
        encoding="utf-8",
    )
    card_header = (
        "lineage,gene,name,matrix,selection_tier,mut_n,wt_n,mut_rate,pass_standard,"
        "pass_strict,oncokb_role,oncokb_class,role_match,is_common_essential,"
        "locus_type,location,gene_group,interpretation,warning\n"
    )
    (liver / "data" / "anchor_gene_cards_standard.csv").write_text(
        card_header
        + "Liver,TP53,tumor protein p53,Damaging,A_role_matched_strict,18,7,0.72,TRUE,TRUE,TSG,TSG_dam,TRUE,FALSE,gene,,,,\n"
        + "Liver,ROLEOG,oncogene,Damaging,C_exploratory,12,20,0.38,TRUE,TRUE,OG,OG_hot,FALSE,FALSE,gene,,,,\n",
        encoding="utf-8",
    )
    candidate_header = card_header
    (liver / "results" / "priority_role_matched_candidates.csv").write_text(
        candidate_header
        + "Liver,TP53,tumor protein p53,Damaging,A_role_matched_strict,18,7,0.72,TRUE,TRUE,TSG,TSG_dam,TRUE,FALSE,gene,,,,\n",
        encoding="utf-8",
    )
    (liver / "results" / "strict_functional_candidates.csv").write_text(
        candidate_header
        + "Liver,TP53,tumor protein p53,Damaging,A_role_matched_strict,18,7,0.72,TRUE,TRUE,TSG,TSG_dam,TRUE,FALSE,gene,,,,\n"
        + "Liver,TERT,telomerase,Hotspot,B_strict_unclassified,16,9,0.64,TRUE,TRUE,,,FALSE,FALSE,gene,,,,\n",
        encoding="utf-8",
    )
    (liver / "results" / "functional_candidates.csv").write_text(
        candidate_header
        + "Liver,TP53,tumor protein p53,Damaging,A_role_matched_strict,18,7,0.72,TRUE,TRUE,TSG,TSG_dam,TRUE,FALSE,gene,,,,\n",
        encoding="utf-8",
    )
    (official / "manifest.json").write_text(
        json.dumps(
            {
                "status": "complete",
                "lineage": "Liver",
                "primary_metric": "Chronos CRISPR Gene Effect",
                "tested_pair_count": 3,
            }
        ),
        encoding="utf-8",
    )
    (official / "anchor_summary.csv").write_text(
        "lineage,anchor_gene,event_type,selection_tier,oncokb_role,base_mut_n,base_wt_n,"
        "tested_target_count,official_default_hit_count,strict_fdr_hit_count\n"
        "Liver,TP53,Damaging,A_role_matched_strict,TSG,18,7,3,0,0\n",
        encoding="utf-8",
    )
    (official / "top_dependency_hits.csv").write_text(
        "scope,lineage,anchor_gene,event_type,dependency_gene,n_mut,n_wt,"
        "delta_gene_effect,fdr_by_anchor,official_default_hit\n"
        "lineage,Liver,TP53,Damaging,SCD,18,7,0.61,0.03,False\n",
        encoding="utf-8",
    )
    pq.write_table(
        pa.table(
            {
                "scope": ["lineage", "lineage", "lineage"],
                "lineage": ["Liver", "Liver", "Liver"],
                "anchor_gene": ["TP53", "TP53", "TP53"],
                "event_type": ["Damaging", "Damaging", "Damaging"],
                "dependency_gene": ["SCD", "GPX4", "ZZZ3"],
                "n_mut": [18, 18, 18],
                "n_wt": [7, 7, 7],
                "mean_gene_effect_mut": [-0.54, -1.2, -0.2],
                "mean_gene_effect_wt": [-1.15, -0.4, -0.1],
                "delta_gene_effect": [0.61, -0.8, -0.1],
                "p_value": [1e-5, 2e-4, 0.4],
                "fdr_by_anchor": [0.03, 0.08, 0.9],
                "official_default_hit": [False, False, False],
            }
        ),
        official / "all_pairs.parquet",
    )


def write_lineage_selectivity_fixtures(root: Path) -> None:
    tests = root / "depmap-26q1-core" / "lineage_dependency_tests"
    tests.mkdir(parents=True, exist_ok=True)
    (tests / "manifest.json").write_text(
        json.dumps({
            "status": "complete",
            "release": "26Q1",
            "lineage_count": 2,
            "model_set_fingerprint": "lineage_dependency_fixture_models",
        }),
        encoding="utf-8",
    )
    (root / "depmap-26q1-core" / "common_essential_genes.csv").write_text(
        "symbol\nDROPCE\n", encoding="utf-8"
    )
    (tests / "dependency_confounder_qc_manifest.json").write_text(
        json.dumps(
            {
                "release": "26Q1",
                "model_set": "lineage_dependency_fixture_models",
                "expression_floor_log2_tpm_plus_1": 1.0,
                "copy_number_amplification_floor_log2": 0.5,
            }
        ),
        encoding="utf-8",
    )
    (tests / "dependency_confounder_qc.csv").write_text(
        "lineage,symbol,expression_median_log2_tpm_plus_1,copy_number_mean_log2\n"
        "Myeloid,KEEP,0.2,0.8\n"
        "Myeloid,DROPCE,5.0,0.0\n",
        encoding="utf-8",
    )
    decoys = [f"DECOY{i:03d}" for i in range(120)]
    symbols = ["KEEP", "DROPCE", *decoys, "BELOW", "UNTESTED"]
    n = len(symbols)
    fdr = [0.01, 0.01] + [0.01] * len(decoys) + [0.2, 1.0]
    delta = [-0.8, -0.8] + [-0.8] * len(decoys) + [-0.01, -0.8]
    pq.write_table(
        pa.table(
            {
                "symbol": symbols,
                "lineage": ["Myeloid"] * n,
                "test_status": ["tested"] * (n - 1) + ["untested"],
                "lineage_n": [40] * n,
                "rest_n": [200] * n,
                "effect_mean_lineage": [-0.5, -1.5] + [-1.0] * (n - 2),
                "effect_mean_rest": [-0.2] * n,
                "effect_mean_difference": delta,
                "fdr_lineage_more_dependent": fdr,
                "rank_more_dependent": list(range(1, n + 1)),
            }
        ),
        tests / "01_Myeloid.parquet",
    )
    pq.write_table(
        pa.table(
            {
                "symbol": ["KEEP", "OTHER"],
                "lineage": ["Breast", "Breast"],
                "test_status": ["tested", "tested"],
                "lineage_n": [50, 50],
                "rest_n": [180, 180],
                "effect_mean_lineage": [-0.3, -1.1],
                "effect_mean_rest": [-0.2, -0.1],
                "effect_mean_difference": [-0.1, -1.0],
                "fdr_lineage_more_dependent": [0.4, 0.01],
                "rank_more_dependent": [80, 1],
            }
        ),
        tests / "02_Breast.parquet",
    )
    pq.write_table(
        pa.table(
            {
                "model_id": ["ACH-000001", "ACH-000002", "ACH-000003", "ACH-000001"],
                "symbol": ["KEEP", "KEEP", "KEEP", "OTHER"],
                "gene_effect": [-1.2, -0.4, -0.8, -2.0],
            }
        ),
        root / "depmap-26q1-core" / "model_gene_effect.parquet",
    )
    pq.write_table(
        pa.table(
            {
                "model_id": ["ACH-000001", "ACH-000002", "ACH-000003"],
                "cell_line_name": ["Fixture A", "Fixture B", "Fixture C"],
                "lineage": ["Myeloid", "Breast", "Myeloid"],
            }
        ),
        root / "depmap-26q1-core" / "model_metadata.parquet",
    )


class LineageMutationQueryTests(DepMapApiTests):
    def setUp(self):
        super().setUp()
        write_lineage_mutation_fixtures(self.settings.knowledge_root)

    def _query(self, payload):
        with TestClient(create_app(self.settings)) as client:
            return client.post(
                "/api/v1/query", headers=self.headers, json=payload
            ).json()

    def test_exact_ptk7_liver_lookup_returns_too_few_mut_not_topn_absence(self):
        payload = self._query(
            {
                "mode": "mutation_anchor",
                "lineage": "肝癌",
                "gene": "PTK7",
                "event": "damaging",
                "limit": 1,
            }
        )
        self.assertEqual(payload["status"], "INELIGIBLE")
        self.assertEqual(payload["rejection_reason"], "TOO_FEW_MUT")
        self.assertEqual(payload["eligibility"]["mut_n"], 1)
        self.assertEqual(payload["eligibility"]["wt_n"], 24)
        self.assertFalse(payload["eligibility"]["criteria"]["standard_mut"]["pass"])
        self.assertTrue(payload["eligibility"]["criteria"]["standard_wt"]["pass"])

    def test_exact_lookup_reports_too_few_wt_role_mismatch_absent_event_and_eligible(self):
        wt_fail = self._query(
            {
                "mode": "mutation_anchor",
                "lineage": "Liver",
                "gene": "WTFAIL",
                "event": "damaging",
            }
        )
        self.assertEqual(wt_fail["status"], "INELIGIBLE")
        self.assertEqual(wt_fail["rejection_reason"], "TOO_FEW_WT")
        role = self._query(
            {
                "mode": "mutation_anchor",
                "lineage": "Liver",
                "gene": "ROLEOG",
                "event": "damaging",
                "anchor_tier": "priority",
            }
        )
        self.assertEqual(role["status"], "NOT_RETAINED")
        self.assertEqual(role["rejection_reason"], "ROLE_MISMATCH")
        self.assertEqual(role["eligibility"]["mut_n"], 12)
        absent = self._query(
            {
                "mode": "mutation_anchor",
                "lineage": "Liver",
                "gene": "PTK7",
                "event": "hotspot",
            }
        )
        self.assertEqual(absent["status"], "NOT_OBSERVED")
        self.assertEqual(absent["rejection_reason"], "ABSENT_EVENT")
        missing = self._query(
            {
                "mode": "mutation_anchor",
                "lineage": "Liver",
                "gene": "NOSUCHGENE",
                "event": "damaging",
            }
        )
        self.assertEqual(missing["status"], "NOT_OBSERVED")
        eligible = self._query(
            {
                "mode": "mutation_anchor",
                "lineage": "Liver",
                "gene": "TP53",
                "event": "damaging",
            }
        )
        self.assertEqual(eligible["status"], "FOUND")
        self.assertEqual(eligible["eligibility"]["mut_n"], 18)
        self.assertIsNone(eligible["rejection_reason"])

    def test_exact_lookup_coverage_states_do_not_use_ssh(self):
        gap = self._query(
            {"mode": "mutation_anchor", "lineage": "Lung", "gene": "EGFR", "event": "damaging"}
        )
        self.assertEqual(gap["status"], "COVERAGE_GAP")
        missing = self._query(
            {"mode": "mutation_anchor", "lineage": "Skin", "gene": "BRAF", "event": "hotspot"}
        )
        self.assertEqual(missing["status"], "MODULE_UNAVAILABLE")

    def test_lineage_mutation_dependency_does_not_scan_pairs_for_ineligible_anchor(self):
        payload = self._query(
            {
                "mode": "lineage_mutation_dependency",
                "lineage": "Liver",
                "source": "PTK7",
                "event": "damaging",
                "limit": 5,
            }
        )
        self.assertEqual(payload["status"], "INELIGIBLE")
        self.assertEqual(payload["provider"], "lineage_official_gene_effect_v2")
        self.assertEqual(payload["rejection_reason"], "TOO_FEW_MUT")
        self.assertEqual(payload["rows"][0]["gene"], "PTK7")
        self.assertNotIn("dependency_gene", payload["rows"][0])

    def test_lineage_mutation_dependency_exact_pair_is_not_topn_bound(self):
        found = self._query(
            {
                "mode": "lineage_mutation_dependency",
                "lineage": "Liver",
                "source": "TP53",
                "target": "GPX4",
                "event": "damaging",
                "limit": 1,
            }
        )
        self.assertEqual(found["status"], "FOUND")
        self.assertEqual(found["rows"][0]["dependency_gene"], "GPX4")
        self.assertEqual(found["rows"][0]["n_mut"], 18)
        self.assertEqual(found["matched_row_count"], 1)
        missing = self._query(
            {
                "mode": "lineage_mutation_dependency",
                "lineage": "Liver",
                "source": "TP53",
                "target": "ABSENTTARGET",
                "event": "damaging",
            }
        )
        self.assertEqual(missing["status"], "NOT_RETAINED")
        untested = self._query(
            {
                "mode": "lineage_mutation_dependency",
                "lineage": "Liver",
                "source": "AXIN1",
                "event": "damaging",
            }
        )
        self.assertEqual(untested["status"], "NOT_COMPUTED")


class LineageSelectivityQueryTests(DepMapApiTests):
    def setUp(self):
        super().setUp()
        write_lineage_selectivity_fixtures(self.settings.knowledge_root)

    def _query(self, payload):
        with TestClient(create_app(self.settings)) as client:
            response = client.post("/api/v1/query", headers=self.headers, json=payload)
        self.assertEqual(response.status_code, 200)
        return response.json()

    def test_exact_gene_is_not_inferred_from_topn(self):
        below = self._query(
            {
                "mode": "lineage_dependency",
                "lineage": "Myeloid",
                "gene": "BELOW",
                "ranking": "selective",
                "limit": 5,
            }
        )
        self.assertEqual(below["status"], "NOT_RETAINED")
        self.assertEqual(below["rows"][0]["symbol"], "BELOW")
        self.assertGreater(below["rows"][0]["rank_more_dependent"], 100)
        found = self._query(
            {
                "mode": "lineage_dependency",
                "lineage": "Myeloid",
                "gene": "KEEP",
                "ranking": "selective",
            }
        )
        self.assertEqual(found["status"], "FOUND")
        missing = self._query(
            {
                "mode": "lineage_dependency",
                "lineage": "Myeloid",
                "gene": "ABSENTGENE",
                "ranking": "selective",
            }
        )
        self.assertEqual(missing["status"], "NOT_TESTED")

    def test_mean_dependency_orders_by_lineage_mean_not_selective_rank(self):
        payload = self._query(
            {
                "mode": "lineage_dependency",
                "lineage": "Myeloid",
                "ranking": "mean_dependency",
                "limit": 1,
            }
        )
        self.assertEqual(payload["rows"][0]["symbol"], "DROPCE")
        self.assertEqual(payload["rows"][0]["effect_mean_lineage"], -1.5)

    def test_model_gene_effect_is_bounded_exact_and_modelid_keyed(self):
        payload = self._query(
            {
                "mode": "model_gene_effect",
                "gene": "keep",
                "lineage": "髓系",
                "gene_effect_at_or_below": -0.9,
                "limit": 1,
            }
        )
        self.assertEqual(payload["status"], "FOUND")
        self.assertEqual(payload["matched_row_count"], 1)
        self.assertEqual(payload["rows"][0]["model_id"], "ACH-000001")
        self.assertEqual(payload["rows"][0]["cell_line_name"], "Fixture A")
        self.assertEqual(
            payload["semantics"]["metric"], "chronos_gene_effect_model_score"
        )
        self.assertEqual(
            payload["semantics"]["threshold_policy"],
            "explicit_less_than_or_equal",
        )

        bounded = self._query(
            {
                "mode": "model_gene_effect",
                "gene": "KEEP",
                "lineage": "Myeloid",
                "limit": 1,
            }
        )
        self.assertEqual(bounded["returned_count"], 1)
        self.assertEqual(bounded["matched_row_count"], 2)
        self.assertEqual(bounded["semantics"]["threshold_policy"], "no_threshold")

        exact = self._query(
            {
                "mode": "model_gene_effect",
                "gene": "KEEP",
                "model_id": "ach-000003",
            }
        )
        self.assertEqual(exact["rows"][0]["model_id"], "ACH-000003")

    def test_model_gene_effect_rejects_invalid_lineage_and_modelid(self):
        with TestClient(create_app(self.settings)) as client:
            bad_lineage = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={"mode": "model_gene_effect", "gene": "KEEP", "lineage": "not-a-lineage"},
            )
            bad_model = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={"mode": "model_gene_effect", "gene": "KEEP", "model_id": "Fixture A"},
            )
        self.assertEqual(bad_lineage.status_code, 200)
        self.assertEqual(bad_lineage.json()["status"], "INELIGIBLE")
        self.assertEqual(bad_model.status_code, 422)

    def test_common_essential_filter_is_one_reader_join(self):
        unfiltered = self._query(
            {
                "mode": "lineage_dependency",
                "lineage": "Myeloid",
                "ranking": "selective",
                "limit": 20,
            }
        )
        symbols = [row["symbol"] for row in unfiltered["rows"]]
        self.assertIn("DROPCE", symbols)
        filtered = self._query(
            {
                "mode": "lineage_dependency",
                "lineage": "Myeloid",
                "ranking": "selective",
                "exclude_common_essential": True,
                "common_essential_source": "depmap_26q1",
                "limit": 20,
            }
        )
        self.assertNotIn("DROPCE", [row["symbol"] for row in filtered["rows"]])
        self.assertTrue(filtered["common_essential_filter_applied"])
        self.assertEqual(filtered["common_essential_removed_count"], 1)
        self.assertGreater(unfiltered["matched_row_count"], unfiltered["returned_count"])
        labeled = self._query(
            {
                "mode": "lineage_dependency",
                "lineage": "Myeloid",
                "gene": "DROPCE",
                "ranking": "selective",
            }
        )
        self.assertEqual(labeled["status"], "FOUND")
        self.assertTrue(labeled["rows"][0]["is_common_essential"])
        excluded = self._query(
            {
                "mode": "lineage_dependency",
                "lineage": "Myeloid",
                "gene": "DROPCE",
                "ranking": "selective",
                "exclude_common_essential": True,
            }
        )
        self.assertEqual(excluded["status"], "NOT_RETAINED")
        self.assertTrue(excluded["rows"][0]["is_common_essential"])

    def test_dependency_confounder_qc_is_typed_and_never_filters(self):
        flagged = self._query(
            {
                "mode": "lineage_dependency",
                "lineage": "Myeloid",
                "gene": "KEEP",
                "ranking": "selective",
            }
        )
        qc = flagged["rows"][0]["dependency_confounder_qc"]
        self.assertEqual(qc["status"], "AVAILABLE")
        self.assertEqual(qc["release"], "26Q1")
        self.assertEqual(
            qc["flags"], ["low_expression", "copy_number_effect"]
        )
        self.assertFalse(qc["filter_applied"])
        self.assertIn("low_expression", flagged["rows"][0]["qc_annotations"])
        self.assertIn("copy_number_effect", flagged["rows"][0]["qc_annotations"])
        self.assertTrue(any(
            path.endswith("dependency_confounder_qc.csv")
            for path in flagged["provenance"]
        ))
        self.assertTrue(any(
            path.endswith("dependency_confounder_qc_manifest.json")
            for path in flagged["provenance"]
        ))

        unflagged = self._query(
            {
                "mode": "lineage_dependency",
                "lineage": "Myeloid",
                "gene": "DROPCE",
                "ranking": "selective",
            }
        )
        self.assertEqual(
            unflagged["rows"][0]["dependency_confounder_qc"]["flags"], []
        )

        unavailable = self._query(
            {
                "mode": "lineage_dependency",
                "lineage": "Myeloid",
                "gene": "BELOW",
                "ranking": "selective",
            }
        )
        self.assertEqual(
            unavailable["rows"][0]["dependency_confounder_qc"]["status"],
            "ANNOTATION_UNAVAILABLE",
        )
        self.assertEqual(
            unavailable["rows"][0]["dependency_confounder_qc"]["flags"], []
        )

        manifest_path = (
            self.settings.knowledge_root
            / "depmap-26q1-core"
            / "lineage_dependency_tests"
            / "dependency_confounder_qc_manifest.json"
        )
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        manifest["release"] = "stale-release"
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        stale = self._query(
            {
                "mode": "lineage_dependency",
                "lineage": "Myeloid",
                "gene": "KEEP",
                "ranking": "selective",
            }
        )
        self.assertEqual(
            stale["rows"][0]["dependency_confounder_qc"]["status"],
            "ANNOTATION_UNAVAILABLE",
        )

        manifest["release"] = "26Q1"
        manifest["model_set"] = "different_fixture_models"
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        mismatched = self._query(
            {
                "mode": "lineage_dependency",
                "lineage": "Myeloid",
                "gene": "KEEP",
                "ranking": "selective",
            }
        )
        self.assertEqual(
            mismatched["rows"][0]["dependency_confounder_qc"]["status"],
            "ANNOTATION_UNAVAILABLE",
        )
        self.assertFalse(any(
            path.endswith("dependency_confounder_qc.csv")
            for path in mismatched["provenance"]
        ))

    def test_cross_lineage_exact_gene_states(self):
        payload = self._query(
            {"mode": "pan_cancer_dependency", "gene": "KEEP", "ranking": "selective"}
        )
        states = {item["lineage"]: item["association_status"] for item in payload["lineages"]}
        self.assertEqual(states["Myeloid"], "FOUND")
        self.assertEqual(states["Breast"], "NOT_RETAINED")


def write_tf_activity_fixtures(root: Path) -> Path:
    module = (
        root
        / "analysis-modules"
        / "转录因子活性-CRISPR基因依赖相关性分析"
        / "results"
        / "tf_activity_dependency_26Q1_v2"
    )
    module.mkdir(parents=True, exist_ok=True)
    (module / "manifest.json").write_text(
        json.dumps({"status": "complete", "release": "26Q1", "tf_count": 4}),
        encoding="utf-8",
    )
    (module / "tf_order.csv").write_text("TF\nMYC\nAHR\nSTAT3\nATF5\n", encoding="utf-8")
    (module / "target_gene_order.csv").write_text(
        "symbol\nZFP36L1\nGPX4\nILK\n", encoding="utf-8"
    )
    with gzip.open(module / "top_hits.csv.gz", "wt", encoding="utf-8", newline="") as handle:
        handle.write(
            "TF,target_gene,direction,rank,correlation,fdr\n"
            "STAT3,ZFP36L1,negative,1,-0.4,0.01\n"
            "STAT3,ILK,negative,2,-0.3,0.02\n"
            "ATF5,ZFP36L1,positive,1,0.35,0.04\n"
        )
    return module


class TfActivityReaderTests(DepMapApiTests):
    def setUp(self):
        super().setUp()
        write_tf_activity_fixtures(self.settings.knowledge_root)

    def _query(self, payload):
        with TestClient(create_app(self.settings)) as client:
            response = client.post("/api/v1/query", headers=self.headers, json=payload)
        self.assertEqual(response.status_code, 200)
        return response.json()

    def test_catalog_valid_tf_never_returns_http_error(self):
        myc = self._query({"mode": "tf_dependency", "source": "MYC", "limit": 5})
        self.assertEqual(myc["status"], "NOT_RETAINED")
        self.assertEqual(myc["entity_class"], "tf_activity")
        self.assertNotEqual(myc["status"], "QUERY_ERROR")
        found = self._query({"mode": "tf_dependency", "source": "STAT3", "limit": 5})
        self.assertEqual(found["status"], "FOUND")
        self.assertEqual(found["rows"][0]["target_gene"], "ZFP36L1")
        atf5 = self._query({"mode": "tf_dependency", "source": "atf5"})
        self.assertEqual(atf5["status"], "FOUND")
        ahr = self._query({"mode": "tf_dependency", "source": "AHR", "target": "GPX4"})
        self.assertEqual(ahr["status"], "NOT_RETAINED")
        self.assertEqual(ahr["source"], "AHR")

    def test_absent_tf_is_coverage_status_not_server_error(self):
        missing = self._query({"mode": "tf_dependency", "source": "NOTATF"})
        self.assertEqual(missing["status"], "NOT_OBSERVED")
        untested = self._query(
            {"mode": "tf_dependency", "source": "STAT3", "target": "ABSENTTARGET"}
        )
        self.assertEqual(untested["status"], "NOT_TESTED")

    def test_universe_and_bulk_ranking_are_bounded_query_surfaces(self):
        universe = self._query({"mode": "tf_dependency", "view": "universe", "limit": 2})
        self.assertEqual(universe["status"], "FOUND")
        self.assertEqual(universe["universe_size"], 4)
        self.assertEqual(universe["matched_row_count"], 4)
        self.assertEqual(universe["returned_count"], 2)
        self.assertEqual(len(universe["rows"]), 2)
        bulk = self._query({"mode": "tf_dependency", "limit": 2})
        self.assertEqual(bulk["matched_row_count"], 3)
        self.assertEqual(bulk["returned_count"], 2)
        self.assertGreater(bulk["matched_row_count"], bulk["returned_count"])

    def test_missing_module_is_unavailable(self):
        import shutil

        shutil.rmtree(
            self.settings.knowledge_root
            / "analysis-modules"
            / "转录因子活性-CRISPR基因依赖相关性分析"
        )
        payload = self._query({"mode": "tf_dependency", "source": "MYC"})
        self.assertEqual(payload["status"], "MODULE_UNAVAILABLE")


if __name__ == "__main__":
    unittest.main()
