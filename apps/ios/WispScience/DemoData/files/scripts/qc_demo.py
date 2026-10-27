# Synthetic demo only. Displayed by the app, never executed by it.
import csv
from pathlib import Path

rows = list(csv.DictReader(Path("data/qc_metrics.csv").open()))
for row in rows:
    print(row["sample_id"], row["mapped_pct"], "synthetic mapping %")
