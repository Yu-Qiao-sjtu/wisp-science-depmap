import importlib.util
import unittest
from pathlib import Path

import numpy as np
from scipy import stats


ENGINE = Path(__file__).resolve().parents[1] / "depmap_official_mutation_engine.py"
SPEC = importlib.util.spec_from_file_location("depmap_official_mutation_engine", ENGINE)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class OfficialMutationEngineTests(unittest.TestCase):
    def test_pooled_t_matches_depmap_equal_var_implementation(self):
        values = np.array(
            [
                [1.0, 3.0],
                [2.0, 4.0],
                [3.0, 5.0],
                [4.0, 6.0],
                [5.0, 7.0],
                [2.0, 4.0],
                [4.0, 2.0],
                [6.0, 4.0],
                [8.0, 6.0],
                [10.0, 8.0],
            ]
        )
        mutant = np.array([True] * 5 + [False] * 5)
        observed = MODULE.pooled_t_statistics(values, mutant, ~mutant)
        expected = stats.ttest_ind(values[mutant], values[~mutant], axis=0, equal_var=True)
        np.testing.assert_allclose(observed["t_stat"], expected.statistic)
        np.testing.assert_allclose(observed["p"], expected.pvalue)
        np.testing.assert_array_equal(observed["df"], np.array([8, 8]))

    def test_bh_adjustment_is_monotone_in_ranked_p_values(self):
        p_values = np.array([0.01, 0.02, 0.5, np.nan])
        adjusted = MODULE.bh_adjust(p_values)
        np.testing.assert_allclose(adjusted[:3], np.array([0.03, 0.03, 0.5]))
        self.assertTrue(np.isnan(adjusted[3]))

    def test_dependency_probability_threshold_is_strictly_greater_than_half(self):
        probability = np.array(
            [[0.50], [0.51], [0.90], [0.10], [np.nan], [0.20], [0.60], [0.40], [0.50], [0.70]]
        )
        mutant = np.array([True] * 5 + [False] * 5)
        counts = MODULE.dependency_counts(probability, mutant, ~mutant)
        self.assertEqual(int(counts[0][0]), 2)
        self.assertEqual(int(counts[2][0]), 2)
        self.assertEqual(int(counts[1][0]), 2)
        self.assertEqual(int(counts[3][0]), 3)


if __name__ == "__main__":
    unittest.main()
