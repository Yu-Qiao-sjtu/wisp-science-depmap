import json
import gzip
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
    resolve_scientific_entity,
)


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
        self.assertEqual(response.json()["query_contract_version"], 9)
        self.assertEqual(response.json()["knowledge_annotation_schema_version"], 1)
        self.assertIn("lineage_network", response.json()["query_modes"])
        self.assertIn("tcga_expression_survival", response.json()["query_modes"])
        self.assertIn("subtype", response.json()["query_modes"])
        self.assertIn("coamplification", response.json()["query_modes"])
        self.assertIn("true_love", response.json()["query_modes"])
        self.assertIn("synthetic_lethal", response.json()["query_modes"])
        self.assertIn("three_d", response.json()["query_modes"])
        self.assertIn("topic_plan", response.json()["query_modes"])
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
            three_d = client.post(
                "/api/v1/query", headers=self.headers,
                json={"mode": "three_d", "family": "dependency_profiles", "cohort": "three_d_all", "gene": "kras", "limit": 5},
            ).json()
        self.assertEqual(true_love["status"], "FOUND")
        self.assertEqual(true_love["rows"][0]["bootstrap_reciprocal_stability"], 0.94)
        self.assertEqual(synthetic["status"], "FOUND")
        self.assertEqual(synthetic["rows"][0]["target_gene"], "ARID1B")
        self.assertEqual(three_d["status"], "FOUND")
        self.assertEqual(three_d["rows"][0]["mean_gene_effect"], -0.62)

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

    def test_topic_plan_preserves_stemness_and_transcription_factor_slots(self):
        client = TestClient(create_app(self.settings))
        with client:
            response = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "topic_plan",
                    "lineage": "肝癌",
                    "phenotypes": ["tumor_cell_stemness"],
                    "molecular_focus": ["transcription_factor"],
                    "evidence_sources": ["depmap"],
                    "requested_outputs": ["candidate_topics", "feasibility"],
                    "execution_policy": "precomputed_only",
                    "limit": 20,
                },
            )
        self.assertEqual(response.status_code, 200)
        payload = response.json()
        self.assertEqual(payload["state"], "PLAN_READY")
        self.assertEqual(payload["semantic_request"]["disease"]["canonical_lineage"], "Liver")
        self.assertEqual(payload["semantic_request"]["phenotypes"], ["tumor_cell_stemness"])
        self.assertEqual(payload["semantic_request"]["molecular_focus"], ["transcription_factor"])
        self.assertEqual(
            payload["scientific_intent"]["schema_version"],
            "wisp.scientific-intent.v1",
        )
        self.assertEqual(
            payload["scientific_intent"]["entity_sets"],
            ["dorothea_tf_abc"],
        )
        self.assertEqual(
            payload["scientific_intent"]["research_relations"],
            [{
                "subject": {"slot": "molecular_focus", "id": "transcription_factor"},
                "predicate": "candidate_association_with",
                "object": {"slot": "phenotype", "id": "tumor_cell_stemness"},
                "evidence_requirement": "direct_result_or_declared_proxy",
            }],
        )
        self.assertEqual(
            payload["evidence_plan"]["schema_version"],
            "wisp.evidence-plan.v1",
        )
        self.assertGreater(
            payload["evidence_plan"]["summary"]["planned_capability_count"],
            1,
        )
        self.assertTrue(
            payload["evidence_plan"]["execution_contract"]["model_must_not_read_blocks"]
        )
        self.assertEqual(payload["coverage"]["phenotypes"][0]["status"], "NOT_COMPUTED")
        self.assertEqual(payload["coverage"]["intersections"][0]["status"], "NOT_COMPUTED")
        self.assertEqual(
            payload["evidence_buckets"]["new_computation_from_available_inputs"][0]["status"],
            "NEW_COMPUTATION_REQUIRED",
        )
        self.assertFalse(payload["new_analysis_started"])

        annotated = payload["coverage"]["capability_annotations"]
        capability_ids = {item["id"] for item in annotated["capabilities"]}
        self.assertIn("lineage_codependency", capability_ids)
        self.assertIn("lineage_expression_dependency", capability_ids)
        self.assertIn("lineage_cnv_dependency", capability_ids)
        self.assertIn("lineage_prism_association", capability_ids)
        self.assertIn("true_love_gene", capability_ids)
        self.assertIn("observational_synthetic_lethal", capability_ids)
        planned_ids = {
            item["capability_id"] for item in payload["evidence_plan"]["steps"]
        }
        self.assertEqual(planned_ids, capability_ids)
        expression_dependency = next(
            item
            for item in payload["evidence_plan"]["steps"]
            if item["capability_id"] == "lineage_expression_dependency"
        )
        self.assertIn(
            "source_gene",
            expression_dependency["entity_set_projection"]["compatible_roles"],
        )
        self.assertIn(
            "target_gene",
            expression_dependency["entity_set_projection"]["compatible_roles"],
        )
        self.assertEqual(
            annotated["summary"]["unmatched_question_tags"],
            ["stemness_proxy", "tumor_cell_stemness"],
        )
        self.assertEqual(
            payload["coverage"]["intersections"][0]["declared_proxy_statuses"],
            ["NOT_COMPUTED"],
        )
        self.assertEqual(
            payload["evidence_buckets"]["declared_proxy_evidence"][0]["claim_level"],
            "DECLARED_PROXY",
        )
        self.assertTrue(
            all(
                item["note"].startswith("Capability match only")
                for item in payload["evidence_buckets"]["composable_evidence"]
            )
        )

    def test_capability_catalog_expands_tf_as_cross_module_entity_set(self):
        client = TestClient(create_app(self.settings))
        with client:
            response = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "capability_catalog",
                    "lineage": "肝癌",
                    "question_tags": ["transcription_factor"],
                    "entity_sets": ["dorothea_tf_abc"],
                },
            )
        self.assertEqual(response.status_code, 200)
        payload = response.json()
        self.assertEqual(payload["state"], "ANNOTATION_PLAN_READY")
        self.assertEqual(payload["request"]["lineage"], "Liver")
        ids = {item["id"] for item in payload["capabilities"]}
        self.assertIn("lineage_codependency", ids)
        self.assertIn("lineage_expression_correlation", ids)
        self.assertIn("lineage_expression_dependency", ids)
        self.assertIn("lineage_pathway_tf_enrichment", ids)
        self.assertIn("lineage_prism_association", ids)
        self.assertIn("subtype_dependency", ids)
        self.assertIn("three_d_dependency", ids)
        self.assertIn("tcga_expression_survival", ids)
        self.assertNotIn("lineage_sparse_networks.previous_20260829", {
            item["id"] for item in payload["storage_inventory"]
        })
        self.assertTrue(payload["claim_boundary"]["annotation_match_is_not_a_result_hit"])

    def test_topic_plan_compiles_relations_without_phenotype_specific_code(self):
        client = TestClient(create_app(self.settings))
        with client:
            response = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "topic_plan",
                    "lineage": "Liver",
                    "phenotypes": ["drug_resistance"],
                    "molecular_focus": ["drug_response"],
                    "execution_policy": "precomputed_only",
                    "limit": 5,
                },
            )
        self.assertEqual(response.status_code, 200)
        payload = response.json()
        self.assertEqual(
            payload["scientific_intent"]["research_relations"],
            [{
                "subject": {"slot": "molecular_focus", "id": "drug_response"},
                "predicate": "candidate_association_with",
                "object": {"slot": "phenotype", "id": "drug_resistance"},
                "evidence_requirement": "direct_result_or_declared_proxy",
            }],
        )
        self.assertEqual(
            payload["evidence_buckets"]["declared_proxy_evidence"], []
        )

    def test_declared_mechanism_selects_registry_relation_predicate(self):
        client = TestClient(create_app(self.settings))
        with client:
            response = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "topic_plan",
                    "lineage": "Liver",
                    "phenotypes": ["tumor_cell_stemness"],
                    "molecular_focus": ["transcription_factor"],
                    "mechanisms": ["transcriptional_regulation"],
                    "execution_policy": "precomputed_only",
                    "limit": 5,
                },
            )
        self.assertEqual(response.status_code, 200)
        relation = response.json()["scientific_intent"]["research_relations"][0]
        self.assertEqual(relation["predicate"], "candidate_regulator_of")

    def test_unknown_entity_set_is_rejected_before_capability_planning(self):
        response = self.client.post(
            "/api/v1/query",
            headers=self.headers,
            json={
                "mode": "capability_catalog",
                "entity_sets": ["invented_tf_collection"],
            },
        )
        self.assertEqual(response.status_code, 422)
        self.assertIn("unsupported entity_sets", response.text)

    def test_topic_plan_rejects_unknown_canonical_concept_but_preserves_unresolved_text(self):
        unknown = self.client.post(
            "/api/v1/query",
            headers=self.headers,
            json={"mode": "topic_plan", "lineage": "Liver", "phenotypes": ["made_up_state"]},
        )
        self.assertEqual(unknown.status_code, 422)
        unresolved = self.client.post(
            "/api/v1/query",
            headers=self.headers,
            json={
                "mode": "topic_plan",
                "lineage": "Liver",
                "unresolved_concepts": ["自定义稀有细胞状态"],
            },
        )
        self.assertEqual(unresolved.status_code, 200)
        self.assertEqual(self.queries[-1]["unresolved_concepts"], ["自定义稀有细胞状态"])

    def test_cancer_dependency_ranking_is_canonical_and_bounded(self):
        response = self.client.post(
            "/api/v1/query",
            headers=self.headers,
            json={
                "mode": "lineage_dependency",
                "lineage": "乳腺癌",
                "ranking": "selective",
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
                "limit": 11,
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

    def test_scientific_entity_registry_resolves_catalog_entities_and_preserves_unverified_terms(self):
        core = self.settings.knowledge_root / "depmap-26q1-core"
        pq.write_table(
            pa.table(
                {
                    "hgnc_id": ["HGNC:6407"],
                    "symbol": ["KRAS"],
                    "name": ["KRAS proto-oncogene, GTPase"],
                    "alias_symbol": ["K-RAS"],
                    "prev_symbol": ["KRAS2"],
                    "in_crispr_effect": [True],
                    "in_crispr_dependency": [True],
                    "in_analysis_set": [True],
                }
            ),
            core / "gene_catalog.parquet",
        )
        drug_root = (
            self.settings.knowledge_root
            / "depmap-26q1-full"
            / "prism_auc_effect_correlation"
        )
        drug_root.mkdir(parents=True, exist_ok=True)
        (drug_root / "drug_order.csv").write_text(
            "CompoundID,ConditionSampleID,ConditionCompoundName,GeneSymbolOfTargets,TargetOrMechanism,ChEMBLID,PubChemCID\n"
            "DPC-1,BRD:1,Trametinib,MAP2K1;MAP2K2,MEK inhibitor,CHEMBL2103875,11707110\n",
            encoding="utf-8",
        )

        cancer = resolve_scientific_entity(self.settings, "cancer", "肝癌")
        self.assertEqual(cancer["selected"]["canonical_id"], "depmap-lineage:Liver")
        gene = resolve_scientific_entity(self.settings, "gene", "KRAS2")
        self.assertEqual(gene["status"], "RESOLVED")
        self.assertEqual(gene["selected"]["canonical_id"], "HGNC:6407")
        self.assertEqual(gene["selected"]["matched_by"], "prev_symbol")
        drug = resolve_scientific_entity(self.settings, "drug", "trametinib")
        self.assertEqual(drug["selected"]["label"], "Trametinib")
        phenotype = resolve_scientific_entity(
            self.settings, "phenotype", "肿瘤细胞干性"
        )
        self.assertEqual(phenotype["selected"]["label"], "tumor_cell_stemness")
        pathway = resolve_scientific_entity(
            self.settings, "pathway", "TGF beta signaling"
        )
        self.assertEqual(pathway["status"], "NORMALIZED_UNVERIFIED")
        self.assertFalse(pathway["is_scientific_evidence"])
        missing = resolve_scientific_entity(self.settings, "gene", "NOT_A_GENE")
        self.assertEqual(missing["status"], "NOT_FOUND")

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

    def test_lineage_direction_discovery_projects_tf_set_across_analysis_families(self):
        enrichment_root = (
            self.settings.knowledge_root
            / "depmap-26q1-full"
            / "lineage_gene_enrichment"
            / "Liver"
        )
        blocks = enrichment_root / "blocks"
        blocks.mkdir(parents=True)
        self._write_manifest(enrichment_root, status="complete", lineage="Liver")
        pq.write_table(
            pa.table(
                {
                    "source_gene": ["TFX", "METABOLIC_GENE"],
                    "collection": ["DOROTHEA_TF_ABC", "PATHWAY"],
                    "term": ["TFX_targets", "HALLMARK_GLYCOLYSIS"],
                    "enrichment_z": [3.4, 9.8],
                    "p_value": [0.001, 1e-12],
                    "fdr": [0.02, 1e-8],
                    "gene_set_collection": ["DOROTHEA", "HALLMARK"],
                    "lineage": ["Liver", "Liver"],
                }
            ),
            blocks / "part-00001.parquet",
        )
        network_root = (
            self.settings.knowledge_root
            / "depmap-26q1-full"
            / "lineage_sparse_networks"
            / "effect_correlation"
            / "Liver"
        )
        self._write_manifest(
            network_root,
            release="26Q1",
            status="complete",
            family="effect_correlation",
            lineage="Liver",
            lineage_sample_n=50,
        )
        pq.write_table(
            pa.table(
                {
                    "family": ["effect_correlation", "effect_correlation"],
                    "lineage": ["Liver", "Liver"],
                    "source_gene": ["TFX", "NOT_A_TF"],
                    "target_gene": ["PARTNER", "OTHER"],
                    "correlation": [-0.8, -0.95],
                    "pair_n": [50, 50],
                    "p_value": [1e-7, 1e-9],
                    "fdr": [1e-5, 1e-7],
                    "direction": ["negative", "negative"],
                    "reverse_correlation": [-0.8, -0.95],
                    "reciprocal_rank_max": [1, 1],
                    "reciprocal_score": [-0.8, -0.95],
                }
            ),
            network_root / "reciprocal_pairs.parquet",
        )
        with TestClient(create_app(self.settings)) as client:
            response = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "lineage_directions",
                    "lineage": "肝癌",
                    "focus": "transcription_factor",
                    "limit": 5,
                },
            )
        self.assertEqual(response.status_code, 200)
        payload = response.json()
        self.assertEqual(payload["lineage"], "Liver")
        self.assertEqual(payload["requested_focus"], "transcription_factor")
        self.assertEqual(
            payload["entity_set_selection"]["resolved"],
            ["dorothea_tf_abc"],
        )
        self.assertEqual(
            payload["entity_set_selection"]["dorothea_tf_abc_member_count"], 1
        )
        effect = next(
            section
            for section in payload["sections"]
            if section["label"] == "effect_correlation"
        )
        self.assertEqual(effect["returned_candidate_count"], 1)
        self.assertEqual(effect["rows"][0]["source_gene"], "TFX")
        enrichment = next(
            section
            for section in payload["sections"]
            if section["label"] == "pathway_tf_enrichment"
        )
        self.assertEqual(enrichment["returned_candidate_count"], 1)
        self.assertEqual(
            enrichment["rows"][0]["collection"],
            "DOROTHEA_TF_ABC",
        )
        self.assertTrue(
            any(
                candidate["anchors"].get("source_gene") == "TFX"
                for candidate in payload["topic_candidates"]
            )
        )

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

    def test_page_contract_distinguishes_window_from_retained_total_and_continues(self):
        calls = []

        async def paged_runner(_settings, query):
            calls.append(query)
            rows = [{"rank": rank} for rank in range(1, min(query["limit"], 5) + 1)]
            return {
                "mode": "lineage_dependency",
                "status": "FOUND",
                "lineage": "Liver",
                "rows": rows,
                "summary": {"total_retained_rows": 5, "returned_count": len(rows)},
            }

        app = create_app(self.settings, runner=paged_runner)
        with TestClient(app) as client:
            first = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={"mode": "lineage_dependency", "lineage": "Liver", "limit": 2},
            ).json()
            second = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "lineage_dependency",
                    "lineage": "Liver",
                    "limit": 2,
                    "cursor": first["page_info"]["next_cursor"],
                },
            ).json()

        self.assertEqual([row["rank"] for row in first["rows"]], [1, 2])
        self.assertEqual(first["page_info"]["returned_rows"], 2)
        self.assertEqual(first["page_info"]["total_retained_rows"], 5)
        self.assertTrue(first["page_info"]["total_is_exact"])
        self.assertTrue(first["page_info"]["has_more"])
        self.assertEqual(first["page_info"]["analysis_scope"]["lineage"], "Liver")
        self.assertEqual([row["rank"] for row in second["rows"]], [3, 4])
        self.assertTrue(second["page_info"]["has_more"])
        self.assertEqual(calls[0]["limit"], 3)
        self.assertEqual(calls[1]["limit"], 5)

    def test_page_cursor_cannot_be_reused_with_different_filters(self):
        async def paged_runner(_settings, query):
            return {
                "mode": "lineage_dependency",
                "status": "FOUND",
                "rows": [{"rank": rank} for rank in range(query["limit"])],
            }

        app = create_app(self.settings, runner=paged_runner)
        with TestClient(app) as client:
            first = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={"mode": "lineage_dependency", "lineage": "Liver", "limit": 2},
            ).json()
            rejected = client.post(
                "/api/v1/query",
                headers=self.headers,
                json={
                    "mode": "lineage_dependency",
                    "lineage": "Lung",
                    "limit": 2,
                    "cursor": first["page_info"]["next_cursor"],
                },
            )

        self.assertEqual(rejected.status_code, 422)
        self.assertIn("does not match this query", rejected.json()["detail"])


if __name__ == "__main__":
    unittest.main()
