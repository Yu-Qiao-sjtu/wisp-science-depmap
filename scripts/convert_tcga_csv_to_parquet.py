"""Convert one TCGA bridge build CSV into a query-efficient Parquet file."""

from __future__ import annotations

import argparse
from pathlib import Path

import pyarrow.csv as csv
import pyarrow.parquet as parquet


def convert(source: Path, destination: Path, expected_rows: int) -> None:
    if destination.exists():
        raise ValueError(f"destination already exists: {destination}")
    table = csv.read_csv(
        source,
        convert_options=csv.ConvertOptions(strings_can_be_null=True),
    )
    if table.num_rows != expected_rows:
        raise ValueError(
            f"row count mismatch: expected {expected_rows}, found {table.num_rows}"
        )
    symbols = table.column("symbol")
    if symbols.null_count or len(set(symbols.to_pylist())) != expected_rows:
        raise ValueError("symbol column must contain the unique DepMap target universe")

    temporary = destination.with_suffix(destination.suffix + ".building")
    parquet.write_table(
        table,
        temporary,
        compression="zstd",
        row_group_size=2048,
        write_statistics=True,
    )
    temporary.replace(destination)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--expected-rows", type=int, required=True)
    args = parser.parse_args()
    try:
        convert(args.source, args.destination, args.expected_rows)
    except ValueError as error:
        raise SystemExit(str(error)) from error


if __name__ == "__main__":
    main()
