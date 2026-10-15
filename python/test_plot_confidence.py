"""Fixtures for structure-confidence PAE blocks, NPZ loading, and summaries."""

import importlib.util
import io
import json
import struct
import tempfile
import unittest
import zipfile
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch


SCRIPT = (
    Path(__file__).resolve().parents[1]
    / "skills/structure-confidence-interpretation/scripts/plot_confidence.py"
)
SKILL = SCRIPT.parents[1] / "SKILL.md"


def load_module():
    spec = importlib.util.spec_from_file_location("plot_confidence", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def npy_bytes(rows):
    """C-order float64 .npy (version 1.0), so tests can build a real archive."""
    height = len(rows)
    width = len(rows[0])
    header = (
        "{'descr': '<f8', 'fortran_order': False, 'shape': "
        f"({height}, {width}), }}"
    )
    pad = (64 - ((10 + len(header) + 1) % 64)) % 64
    header = header + (" " * pad) + "\n"
    payload = b"".join(struct.pack("<d", float(value)) for row in rows for value in row)
    return b"\x93NUMPY\x01\x00" + struct.pack("<H", len(header)) + header.encode("latin1") + payload


def write_npz(path, arrays):
    with zipfile.ZipFile(path, "w") as archive:
        for name, rows in arrays.items():
            archive.writestr(f"{name}.npy", npy_bytes(rows))


def blank(length, value):
    return [[value for _ in range(length)] for _ in range(length)]


def paint(matrix, rows, cols, value):
    for row in rows:
        for col in cols:
            matrix[row][col] = value


def zero_diagonal(matrix):
    for index in range(len(matrix)):
        matrix[index][index] = 0.0


def hidden_interface_matrix():
    """Eight low-PAE residues in A and two in B, with a high A-B block.

    Within-chain pairs outnumber the interface, so the off-diagonal median
    stays near 2 Å while the chain-pair block is 25 Å.
    """
    matrix = blank(10, 25.0)
    paint(matrix, range(8), range(8), 2.0)
    paint(matrix, range(8, 10), range(8, 10), 2.0)
    zero_diagonal(matrix)
    return matrix


def two_domain_matrix():
    matrix = blank(12, 18.0)
    paint(matrix, range(6), range(6), 2.0)
    paint(matrix, range(6, 12), range(6, 12), 2.0)
    zero_diagonal(matrix)
    return matrix


class PlotConfidenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.mod = load_module()

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="wisp pae ")
        self.addCleanup(temporary.cleanup)
        self.tmp = Path(temporary.name)

    def write_json(self, name, payload):
        path = self.tmp / name
        path.write_text(json.dumps(payload), encoding="utf-8")
        return path

    def run_script(self, *paths):
        buffer = io.StringIO()
        with redirect_stdout(buffer):
            code = self.mod.main(["plot_confidence.py", *(str(path) for path in paths)])
        return code, buffer.getvalue()

    def test_chain_blocks_are_the_interface_not_the_off_diagonal_median(self):
        matrix = hidden_interface_matrix()
        off_diagonal = [
            matrix[row][col]
            for row in range(10)
            for col in range(10)
            if row != col
        ]
        self.assertEqual(self.mod.median(off_diagonal), 2.0)
        for key in ("token_chain_ids", "chain_ids", "asym_id"):
            with self.subTest(key=key):
                lines = self.mod.block_lines(matrix, {
                    key: ["A"] * 8 + ["B"] * 2,
                })
                text = "\n".join(lines)
                self.assertIn(
                    "  interface A-B: median 25.00 Å -> orientation is a guess (>15 Å)",
                    text,
                )
                self.assertIn("  chain A internal: median 2.00 Å", text)
                self.assertIn("  chain B internal: median 2.00 Å", text)
                self.assertNotIn("off-diag", text)
                self.assertNotIn("median(off-diag)", text)

    def test_domain_blocks_are_not_reported_as_a_chain_interface(self):
        matrix = two_domain_matrix()
        text = "\n".join(self.mod.block_lines(matrix, {
            "domain_ids": ["d1"] * 6 + ["d2"] * 6,
        }))
        self.assertIn("  no chain ids; interface PAE not reported", text)
        self.assertIn(
            "  inter-domain d1-d2: median 18.00 Å -> orientation is a guess (>15 Å)",
            text,
        )
        self.assertNotIn("  interface ", text)

    def test_single_chain_infers_domain_blocks_and_skips_interface(self):
        text = "\n".join(self.mod.block_lines(two_domain_matrix(), {
            "token_chain_ids": ["A"] * 12,
        }))
        self.assertIn("  single chain; interface PAE not reported", text)
        self.assertIn("inferred contiguous domains", text)
        self.assertIn("  inter-domain d1-d2: median 18.00 Å -> orientation is a guess (>15 Å)", text)
        self.assertNotIn("  interface A-", text)

    def test_within_chain_domains_stay_separate_from_the_interface(self):
        matrix = blank(12, 3.0)
        paint(matrix, range(4), range(4), 1.0)
        paint(matrix, range(4, 8), range(4, 8), 1.0)
        paint(matrix, range(4), range(4, 8), 20.0)
        paint(matrix, range(4, 8), range(4), 20.0)
        paint(matrix, range(8, 12), range(8, 12), 1.0)
        zero_diagonal(matrix)
        text = "\n".join(self.mod.block_lines(matrix, {
            "token_chain_ids": ["A"] * 8 + ["B"] * 4,
            "domain_ids": ["n"] * 4 + ["c"] * 4 + ["b"] * 4,
        }))
        self.assertIn("  interface A-B: median 3.00 Å -> trust (<5 Å)", text)
        self.assertIn(
            "  inter-domain A:n-c: median 20.00 Å -> orientation is a guess (>15 Å)",
            text,
        )
        self.assertNotIn("  interface n-c", text)

    def test_uniform_matrix_does_not_invent_an_interface(self):
        text = "\n".join(self.mod.block_lines(blank(6, 3.0), {}))
        self.assertIn("  no chain ids; interface PAE not reported", text)
        self.assertIn("  no domain blocks; orientation PAE not reported", text)
        self.assertNotIn("  interface ", text)
        self.assertNotIn("  inter-domain ", text)

    def test_mismatched_chain_ids_are_ignored(self):
        text = "\n".join(self.mod.block_lines(blank(4, 2.0), {
            "token_chain_ids": ["A", "B"],
        }))
        self.assertIn("token_chain_ids does not match the 4x4 PAE matrix", text)
        self.assertIn("  no chain ids; interface PAE not reported", text)

    def test_json_layouts_and_generated_summaries(self):
        matrix = hidden_interface_matrix()
        chains = ["A"] * 8 + ["B"] * 2
        af3 = self.write_json("fold_full_data_0.json", {
            "pae": matrix,
            "token_chain_ids": chains,
            "iptm": 0.91,
        })
        af2 = self.write_json("predicted_aligned_error.json", {
            "predicted_aligned_error": matrix,
            "max_predicted_aligned_error": 31.75,
        })
        nested = self.write_json("scores.json", {
            "predicted_aligned_error": {"predicted_aligned_error": two_domain_matrix()},
            "ptm": 0.4,
        })
        wrapped = self.write_json("server_pae.json", {
            "pae": {"pae": matrix, "max_pae": 31.75},
            "token_chain_ids": chains,
        })
        code, text = self.run_script(af3, af2, nested, wrapped)
        self.assertEqual(code, 0)
        self.assertIn("ipTM (interface): 0.910  -> trust interface (>0.8)", text)
        self.assertEqual(text.count("interface A-B: median 25.00 Å"), 2)
        self.assertIn("pTM (fold): 0.400  -> topology questionable", text)
        self.assertIn("inter-domain d1-d2: median 18.00 Å", text)
        self.assertNotIn("off-diag", text)

    def test_openfold_metrics_without_a_matrix(self):
        path = self.write_json("query_confidences_aggregated.json", {
            "avg_plddt": 78.96,
            "ptm": 0.667,
            "iptm": 0.0,
            "gpde": 0.73,
            "has_clash": 0.0,
            "sample_ranking_score": 0.133,
        })
        code, text = self.run_script(path)
        self.assertEqual(code, 0)
        self.assertIn("avg pLDDT: 78.960  -> usable", text)
        self.assertIn("pTM (fold): 0.667  -> topology OK (>0.5)", text)
        self.assertIn("ipTM (interface): 0.000  -> do NOT trust (<0.6)", text)
        self.assertIn("gpDE (0-1): 0.730  -> usable (closer to 1 is better)", text)
        self.assertIn("clash: 0.000  -> none", text)
        self.assertIn("sample ranking score: 0.133  -> sample ranking only", text)
        self.assertIn("(no PAE matrix in this file - metrics only)", text)
        self.assertNotIn("lower is better", text)

    def test_raw_pde_and_gpde_use_opposite_directions(self):
        path = self.write_json("confidence_model_0.json", {
            "complex_pde": 1.2,
            "complex_ipde": 6.5,
            "gpde": 0.91,
            "confidence_score": 0.8,
            "has_clash": 1,
        })
        code, text = self.run_script(path)
        self.assertEqual(code, 0)
        pde = next(line for line in text.splitlines() if line.startswith("  complex PDE"))
        ipde = next(line for line in text.splitlines() if line.startswith("  complex iPDE"))
        gpde = next(line for line in text.splitlines() if line.startswith("  gpDE"))
        self.assertIn("1.200", pde)
        self.assertIn("lower is better", pde)
        self.assertNotIn("closer to 1", pde)
        self.assertIn("uncertain (>5 Å; lower is better)", ipde)
        self.assertIn("0.910", gpde)
        self.assertIn("closer to 1 is better", gpde)
        self.assertNotIn("lower is better", gpde)
        self.assertIn("CLASH - discard sample", text)
        self.assertIn("ranking score: 0.800  -> sample ranking only", text)

    def test_npz_is_loaded_instead_of_being_decoded_as_json(self):
        matrix = hidden_interface_matrix()
        path = self.tmp / "predicted_aligned_error_model_1_multimer_v3.npz"
        write_npz(path, {"predicted_aligned_error": matrix})
        self.assertFalse(path.read_bytes().lstrip().startswith(b"{"))
        with patch.object(self.mod, "load_json", side_effect=AssertionError("parsed as JSON")):
            try:
                import numpy  # noqa: F401
            except ImportError:
                code, text = self.run_script(path)
                self.assertEqual(code, 0)
                self.assertIn("numpy is required to read NPZ", text)
                self.assertNotIn("not readable JSON", text)
                return
            data = self.mod.load_confidence(path)
        self.assertEqual(data["predicted_aligned_error"], matrix)
        code, text = self.run_script(path)
        self.assertEqual(code, 0)
        self.assertIn("PAE matrix: 10x10", text)
        self.assertIn("no chain ids; interface PAE not reported", text)
        self.assertNotIn("not readable JSON", text)

    @unittest.skipUnless(importlib.util.find_spec("numpy"), "numpy is not installed")
    def test_chai_npz_round_trip_keeps_chain_blocks(self):
        import numpy as np

        matrix = hidden_interface_matrix()
        path = self.tmp / "scores.model_0.npz"
        np.savez(
            path,
            pae=np.asarray(matrix, dtype=np.float32),
            iptm=np.float64(0.55),
            ptm=np.float64(0.81),
            aggregate_score=np.float64(0.6),
            token_chain_ids=np.asarray(["A"] * 8 + ["B"] * 2),
        )
        code, text = self.run_script(path)
        self.assertEqual(code, 0)
        self.assertIn("ipTM (interface): 0.550  -> do NOT trust (<0.6)", text)
        self.assertIn("pTM (fold): 0.810  -> topology OK (>0.5)", text)
        self.assertIn("Chai aggregate score: 0.600  -> sample ranking only", text)
        self.assertIn("interface A-B: median 25.00 Å -> orientation is a guess (>15 Å)", text)

    def test_missing_matplotlib_still_prints_the_summary(self):
        path = self.write_json("with_pae.json", {
            "pae": hidden_interface_matrix(),
            "token_chain_ids": ["A"] * 8 + ["B"] * 2,
        })
        with patch.object(self.mod, "import_pyplot", side_effect=ImportError("no matplotlib")):
            code, text = self.run_script(path)
        self.assertEqual(code, 0)
        self.assertIn("interface A-B: median 25.00 Å", text)
        self.assertIn("matplotlib is not installed", text)
        self.assertIn("pip install matplotlib", text)
        self.assertNotIn("Traceback", text)
        self.assertFalse((self.tmp / "with_pae.json.pae.png").exists())

    def test_heatmap_color_scale_stays_fixed_when_the_matrix_is_small(self):
        recorded = {}

        class Axis:
            def imshow(self, matrix, vmin, vmax, cmap):
                recorded["vmax"] = vmax
                recorded["vmin"] = vmin
                recorded["cmap"] = cmap
                recorded["max"] = max(max(row) for row in matrix)
                return "image"

            def set_title(self, *_args, **_kwargs):
                return None

            def set_xlabel(self, *_args, **_kwargs):
                return None

            def set_ylabel(self, *_args, **_kwargs):
                return None

        class Figure:
            def colorbar(self, *_args, **_kwargs):
                return None

            def tight_layout(self):
                return None

            def savefig(self, path):
                Path(path).write_bytes(b"png")

        class Plt:
            def subplots(self, **_kwargs):
                return Figure(), Axis()

            def close(self, _fig):
                return None

        matrix = [[0.0, 4.0], [4.0, 0.0]]
        out = self.tmp / "small.pae.png"
        with patch.object(self.mod, "import_pyplot", return_value=Plt()):
            self.mod.plot_pae(matrix, out, "small")
        self.assertEqual(recorded["vmin"], 0.0)
        self.assertEqual(recorded["vmax"], 31.75)
        self.assertEqual(recorded["max"], 4.0)
        self.assertTrue(out.is_file())

    def test_unreadable_input_is_skipped(self):
        path = self.tmp / "notes.txt"
        path.write_text("not json", encoding="utf-8")
        code, text = self.run_script(path)
        self.assertEqual(code, 0)
        self.assertIn("[skip] notes.txt: not readable JSON", text)

    def test_skill_keeps_both_low_plddt_explanations_and_splits_pde(self):
        text = SKILL.read_text(encoding="utf-8")
        self.assertIn("unstructured in isolation", text)
        self.assertIn("structured only as part of a complex", text)
        self.assertIn("只在结合伴侣后才折叠", text)
        self.assertIn("也可能只在复合物里才有结构", text)
        self.assertIn("不要断言", text)
        self.assertNotIn("通常是内在无序区", text)
        self.assertNotIn("直接告诉用户\"这里本来就没有固定结构\"", text)
        self.assertIn("complex_pde", text)
        self.assertIn("complex_ipde", text)
        self.assertIn("越低越可信", text)
        self.assertIn("越接近 1 越好", text)
        self.assertIn("它不是原始 PDE", text)
