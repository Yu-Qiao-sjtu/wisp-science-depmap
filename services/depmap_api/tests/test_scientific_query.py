import unittest

from services.depmap_api.scientific_query import (
    bound_after_rank,
    classify_coverage,
    classify_exact_entity,
    criteria_failures,
    filter_before_limit,
)


class ScientificQueryContractTests(unittest.TestCase):
    def test_coverage_precedes_biology(self):
        self.assertEqual(classify_coverage(module_present=False), "MODULE_UNAVAILABLE")
        self.assertEqual(
            classify_coverage(module_present=True, complete=False), "NOT_COMPUTED"
        )
        self.assertEqual(
            classify_coverage(module_present=True, table_present=False), "COVERAGE_GAP"
        )
        self.assertIsNone(classify_coverage(module_present=True, complete=True, table_present=True))

    def test_exact_entity_does_not_use_ranking_absence(self):
        self.assertEqual(classify_exact_entity(observed=False), "NOT_OBSERVED")
        self.assertEqual(
            classify_exact_entity(observed=True, eligible=False), "INELIGIBLE"
        )
        self.assertEqual(
            classify_exact_entity(observed=True, eligible=True, retained=False),
            "NOT_RETAINED",
        )
        self.assertEqual(
            classify_exact_entity(observed=True, eligible=True, retained=True),
            "FOUND",
        )

    def test_filter_applies_before_limit_on_decoy_prefix(self):
        rows = [{"id": f"d{i}"} for i in range(30)] + [{"id": "key"}]
        matched = filter_before_limit(rows, lambda row: row["id"] == "key", limit=1)
        self.assertEqual(matched, [{"id": "key"}])

    def test_rank_then_bound_preserves_match_count(self):
        rows = [{"id": "b", "score": 2}, {"id": "a", "score": 1}, {"id": "c", "score": 3}]
        page, matched = bound_after_rank(rows, key=lambda row: row["score"], limit=1)
        self.assertEqual(matched, 3)
        self.assertEqual(page[0]["id"], "a")

    def test_threshold_failures_use_declared_bounds_not_list_position(self):
        self.assertEqual(
            criteria_failures(observed={"mut": 1, "wt": 24}, required={"mut": 3, "wt": 5}),
            ["TOO_FEW_MUT"],
        )
        self.assertEqual(
            criteria_failures(observed={"mut": 12, "wt": 2}, required={"mut": 3, "wt": 5}),
            ["TOO_FEW_WT"],
        )
        self.assertEqual(
            criteria_failures(observed={"mut": None, "wt": 10}, required={"mut": 3, "wt": 5}),
            ["COUNTS_UNAVAILABLE"],
        )
