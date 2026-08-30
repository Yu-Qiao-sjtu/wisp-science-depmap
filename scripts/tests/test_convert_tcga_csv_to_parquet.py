import tempfile
import unittest
from pathlib import Path

import pyarrow.parquet as parquet

from scripts.convert_tcga_csv_to_parquet import convert


class ConvertTcgaCsvToParquetTests(unittest.TestCase):
    def test_writes_unique_gene_universe_atomically(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "genes.csv"
            destination = root / "genes.parquet"
            source.write_text("symbol,value\nESR1,1.5\nTP53,2.5\n", encoding="utf-8")

            convert(source, destination, expected_rows=2)

            table = parquet.read_table(destination)
            self.assertEqual(table.column("symbol").to_pylist(), ["ESR1", "TP53"])
            self.assertFalse(destination.with_suffix(".parquet.building").exists())

    def test_rejects_duplicate_symbols(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "genes.csv"
            destination = root / "genes.parquet"
            source.write_text("symbol,value\nESR1,1.5\nESR1,2.5\n", encoding="utf-8")

            with self.assertRaisesRegex(ValueError, "unique DepMap target universe"):
                convert(source, destination, expected_rows=2)

            self.assertFalse(destination.exists())


if __name__ == "__main__":
    unittest.main()
