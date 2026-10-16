#!/usr/bin/env python3
"""Generate hash-authenticated AutoDock Vina Python API scoring oracles.

The generator never downloads or installs packages. The caller supplies an
explicit Python interpreter plus the exact Vina and NumPy wheel archives and
trusted SHA256 digests. Archive identity, package versions, and the installed
distributions' direct_url archive provenance are verified before Vina.score()
is called.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
import subprocess
import zipfile
from email.parser import Parser
from pathlib import Path, PurePosixPath
from typing import Any, Callable

EXPECTED_COMMIT = "8eb40404f4f45608acb3b01427587ac049f27c1f"
EXPECTED_METHOD = "vina.Vina.score()"
EXPECTED_VINA_VERSION = "1.2.7"
EXPECTED_NUMPY_VERSION = "1.26.4"
EXPECTED_WHEELS = {
    "vina": {
        "filename": "vina-1.2.7-cp312-cp312-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
        "sha256": "a6e46560e50e693fb29883b82be8d9a4d74207b4464cfedf2bd5235192f4eb3d",
        "version": EXPECTED_VINA_VERSION,
    },
    "numpy": {
        "filename": "numpy-1.26.4-cp312-cp312-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
        "sha256": "675d61ffbfa78604709862923189bad94014bef562cc35cf61d3a07bba02a7ed",
        "version": EXPECTED_NUMPY_VERSION,
    },
}
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
WINDOWS_ABSOLUTE_RE = re.compile(r"^[A-Za-z]:[\\/]")
RAW_VECTOR_COLUMNS = [
    "total",
    "lig_inter",
    "flex_inter",
    "other_inter",
    "flex_intra",
    "lig_intra",
    "torsions",
    "lig_intra_best_pose",
]
EXPECTED_FIELDS = [
    "estimated_free_energy_of_binding_kcal_mol",
    "final_intermolecular_energy_kcal_mol",
    "ligand_receptor_energy_kcal_mol",
    "final_total_internal_energy_kcal_mol",
    "ligand_internal_energy_kcal_mol",
    "torsional_free_energy_kcal_mol",
    "unbound_system_energy_kcal_mol",
]
EXPECTED_MAPPING = {
    EXPECTED_FIELDS[0]: "total",
    EXPECTED_FIELDS[1]: "lig_inter + flex_inter",
    EXPECTED_FIELDS[2]: "lig_inter",
    EXPECTED_FIELDS[3]: "other_inter + flex_intra + lig_intra",
    EXPECTED_FIELDS[4]: "lig_intra",
    EXPECTED_FIELDS[5]: "torsions",
    EXPECTED_FIELDS[6]: "lig_intra_best_pose",
}
RESULT_MARKER = "WISP_VINA_API_RESULT="

WORKER = r'''
import importlib.metadata as metadata
import json
import os
import platform
import sys
from pathlib import PurePosixPath
from urllib.parse import unquote, urlparse

request = json.loads(sys.stdin.read())


def fail(message):
    raise RuntimeError(message)


def package_record(distribution_name, module, expected):
    distribution = metadata.distribution(distribution_name)
    distribution_version = distribution.version
    module_version = str(module.__version__)
    if distribution_version != expected["version"]:
        fail(
            f"{distribution_name} distribution version mismatch: "
            f"expected {expected['version']}, got {distribution_version}"
        )
    if module_version != expected["version"]:
        fail(
            f"{distribution_name} module version mismatch: "
            f"expected {expected['version']}, got {module_version}"
        )

    raw_direct_url = distribution.read_text("direct_url.json")
    if raw_direct_url is None:
        fail(
            f"{distribution_name} has no direct_url.json; reinstall from the "
            "explicit trusted wheel so archive provenance can be verified"
        )
    direct_url = json.loads(raw_direct_url)
    archive_info = direct_url.get("archive_info")
    if not isinstance(archive_info, dict):
        fail(f"{distribution_name} direct_url.json lacks archive_info")
    hashes = archive_info.get("hashes")
    digest = hashes.get("sha256") if isinstance(hashes, dict) else None
    if digest is None:
        legacy_hash = archive_info.get("hash")
        if isinstance(legacy_hash, str) and legacy_hash.startswith("sha256="):
            digest = legacy_hash.removeprefix("sha256=")
    if digest != expected["sha256"]:
        fail(
            f"{distribution_name} installed archive hash mismatch: "
            f"expected {expected['sha256']}, got {digest!r}"
        )

    url = direct_url.get("url")
    if not isinstance(url, str):
        fail(f"{distribution_name} direct_url.json lacks a URL")
    archive_filename = PurePosixPath(unquote(urlparse(url).path)).name
    if archive_filename != expected["filename"]:
        fail(
            f"{distribution_name} installed archive filename mismatch: "
            f"expected {expected['filename']}, got {archive_filename!r}"
        )

    return {
        "distribution_version": distribution_version,
        "module_version": module_version,
        "wheel_filename": expected["filename"],
        "wheel_sha256": expected["sha256"],
        "installation_provenance": {
            "method": "direct_url_archive_hash",
            "wheel_filename": archive_filename,
            "sha256": digest,
        },
    }


try:
    import numpy
    import vina
    from vina import Vina

    packages = {
        "vina": package_record("vina", vina, request["packages"]["vina"]),
        "numpy": package_record("numpy", numpy, request["packages"]["numpy"]),
    }

    engine = Vina(
        sf_name=request["scoring"],
        cpu=request["cpu"],
        seed=request["seed"],
        no_refine=request["no_refine"],
        verbosity=0,
    )
    engine.set_receptor(request["receptor_path"])
    engine.set_ligand_from_file(request["ligand_path"])
    engine.compute_vina_maps(
        center=request["box_center"],
        box_size=request["box_size"],
        spacing=request["spacing"],
        force_even_voxels=request["force_even_voxels"],
    )
    raw_vector = [float(value) for value in engine.score().tolist()]
    if len(raw_vector) != 8:
        fail(f"Vina.score() returned {len(raw_vector)} columns instead of 8")

    info = engine.info()
    if info["scoring_function"] != request["scoring"]:
        fail("Vina.info() scoring function differs from the request")
    if list(info["box_center"]) != request["box_center"]:
        fail("Vina.info() box center differs from the request")
    if list(info["box_size"]) != request["box_size"]:
        fail("Vina.info() box size differs from the request")
    if float(info["box_spacing"]) != request["spacing"]:
        fail("Vina.info() spacing differs from the request")
    if int(info["seed"]) != request["seed"]:
        fail("Vina.info() seed differs from the request")
    if bool(info["no_refine"]) != request["no_refine"]:
        fail("Vina.info() no_refine differs from the request")

    response = {
        "runtime": {
            "python_implementation": platform.python_implementation(),
            "python_version": platform.python_version(),
        },
        "packages": packages,
        "weights": [float(value) for value in info["weights"]],
        "raw_vector_kcal_mol": raw_vector,
    }
    print("WISP_VINA_API_RESULT=" + json.dumps(response, separators=(",", ":")))
except Exception as error:
    print(f"Vina Python API worker failed: {error}", file=sys.stderr)
    raise
'''


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def git_blob_oid(payload: bytes) -> str:
    header = f"blob {len(payload)}\0".encode("ascii")
    return hashlib.sha1(header + payload).hexdigest()


def verify_official_git_blob(entry: dict[str, Any], payload: bytes) -> None:
    if entry.get("origin") != "official_upstream":
        return
    source = entry.get("source")
    if not isinstance(source, dict):
        raise ValueError(f"{entry.get('id', '<unknown>')}: official source metadata missing")
    expected = source.get("git_blob")
    if expected is None:
        return
    if not isinstance(expected, str) or re.fullmatch(r"[0-9a-f]{40}", expected) is None:
        raise ValueError(f"{entry['id']}: invalid recorded Git blob OID")
    actual = git_blob_oid(payload)
    if actual != expected:
        raise ValueError(
            f"{entry['id']}: Git blob OID mismatch: expected {expected}, got {actual}"
        )


def safe_relative_path(value: str) -> PurePosixPath:
    pure = PurePosixPath(value)
    if pure.is_absolute() or ".." in pure.parts or "\\" in value:
        raise ValueError(f"unsafe fixture-relative path {value!r}")
    return pure


def fixture_entry(manifest: dict[str, Any], fixture_id: str) -> dict[str, Any]:
    entry = next((value for value in manifest["fixtures"] if value["id"] == fixture_id), None)
    if entry is None:
        raise ValueError(f"unknown fixture id {fixture_id!r}")
    return entry


def fixture_path(root: Path, manifest: dict[str, Any], fixture_id: str) -> Path:
    entry = fixture_entry(manifest, fixture_id)
    pure = safe_relative_path(entry["path"])
    return root.joinpath(*pure.parts)


def preflight_selected_cases(
    root: Path,
    manifest: dict[str, Any],
    selected: set[str],
) -> list[dict[str, Any]]:
    prepared: list[dict[str, Any]] = []
    for case in manifest["scoring_cases"]:
        if selected and case["id"] not in selected:
            continue
        receptor_entry = fixture_entry(manifest, case["receptor_fixture"])
        ligand_entry = fixture_entry(manifest, case["ligand_fixture"])
        receptor = fixture_path(root, manifest, case["receptor_fixture"])
        ligand = fixture_path(root, manifest, case["ligand_fixture"])
        inputs = (
            ("receptor", receptor_entry, receptor),
            ("ligand", ligand_entry, ligand),
        )
        for role, entry, path in inputs:
            if not path.is_file():
                raise ValueError(f"{case['id']}: {role} fixture is missing")
            payload = path.read_bytes()
            verify_official_git_blob(entry, payload)
            actual_sha256 = hashlib.sha256(payload).hexdigest()
            if actual_sha256 != entry["sha256"]:
                raise ValueError(
                    f"{case['id']}: {role} fixture SHA256 differs from manifest"
                )
            if len(payload) != entry["bytes"]:
                raise ValueError(
                    f"{case['id']}: {role} fixture byte length differs from manifest"
                )
        relative = safe_relative_path(case["oracle_output_path"])
        prepared.append(
            {
                "case": case,
                "receptor_entry": receptor_entry,
                "ligand_entry": ligand_entry,
                "receptor": receptor,
                "ligand": ligand,
                "output_relative": relative,
                "destination": root.joinpath(*relative.parts),
            }
        )
    return prepared


def wheel_metadata(path: Path) -> tuple[str, str]:
    try:
        with zipfile.ZipFile(path) as archive:
            metadata_names = [
                name for name in archive.namelist() if name.endswith(".dist-info/METADATA")
            ]
            if len(metadata_names) != 1:
                raise ValueError(
                    f"{path.name}: expected exactly one .dist-info/METADATA file"
                )
            metadata_text = archive.read(metadata_names[0]).decode("utf-8")
    except (OSError, UnicodeDecodeError, zipfile.BadZipFile, KeyError) as error:
        raise ValueError(f"cannot inspect wheel {path.name}: {error}") from error
    message = Parser().parsestr(metadata_text)
    name = message.get("Name")
    version = message.get("Version")
    if not name or not version:
        raise ValueError(f"{path.name}: wheel metadata lacks Name or Version")
    return name, version


def verify_wheel(
    parser: argparse.ArgumentParser,
    package: str,
    path: Path,
    supplied_sha256: str,
) -> dict[str, str]:
    expected = EXPECTED_WHEELS[package]
    if not path.is_file():
        parser.error(f"--{package}-wheel-path is not a file: {path}")
    supplied = supplied_sha256.lower()
    if SHA256_RE.fullmatch(supplied) is None:
        parser.error(
            f"--{package}-wheel-sha256 must be exactly 64 lowercase hexadecimal characters"
        )
    if supplied != expected["sha256"]:
        parser.error(
            f"{package} trusted SHA256 differs from the pinned PyPI digest: "
            f"expected {expected['sha256']}, got {supplied}"
        )
    if path.name != expected["filename"]:
        parser.error(
            f"{package} wheel filename differs: expected {expected['filename']!r}, "
            f"got {path.name!r}"
        )
    actual = sha256(path)
    if actual != supplied:
        parser.error(
            f"{package} wheel SHA256 mismatch: expected {supplied}, got {actual}"
        )
    try:
        metadata_name, metadata_version = wheel_metadata(path)
    except ValueError as error:
        parser.error(str(error))
    if metadata_name.lower() != package or metadata_version != expected["version"]:
        parser.error(
            f"{package} wheel metadata mismatch: expected {package} "
            f"{expected['version']}, got {metadata_name} {metadata_version}"
        )
    return dict(expected)


def local_python_launcher(
    parser: argparse.ArgumentParser,
    python_path: str,
) -> tuple[list[str], Callable[[Path], str]]:
    if os.name == "nt" and python_path.startswith("/"):
        probe = subprocess.run(
            ["wsl.exe", "--exec", "test", "-f", python_path],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        if probe.returncode != 0:
            parser.error(f"--vina-python-path is not a WSL file: {python_path}")

        def to_wsl_path(path: Path) -> str:
            result = subprocess.run(
                ["wsl.exe", "--exec", "wslpath", "-a", str(path.resolve())],
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                check=False,
            )
            if result.returncode != 0 or not result.stdout.strip():
                parser.error(
                    f"could not translate path for WSL: {path}\n{result.stderr.strip()}"
                )
            return result.stdout.strip()

        return ["wsl.exe", "--exec", python_path], to_wsl_path

    local = Path(python_path).expanduser()
    if local.is_file():
        resolved = local.resolve()
        return [str(resolved)], lambda path: str(path.resolve())

    parser.error(f"--vina-python-path is not a file: {python_path}")
    raise AssertionError("argparse.error always exits")


def clean_energy(value: float) -> float:
    rounded = round(float(value), 3)
    return 0.0 if rounded == 0 else rounded


def map_expected(raw: list[float]) -> dict[str, float]:
    if len(raw) != 8 or not all(math.isfinite(value) for value in raw):
        raise ValueError("Vina.score() must return eight finite values")
    values = [clean_energy(value) for value in raw]
    return {
        EXPECTED_FIELDS[0]: values[0],
        EXPECTED_FIELDS[1]: clean_energy(values[1] + values[2]),
        EXPECTED_FIELDS[2]: values[1],
        EXPECTED_FIELDS[3]: clean_energy(values[3] + values[4] + values[5]),
        EXPECTED_FIELDS[4]: values[5],
        EXPECTED_FIELDS[5]: values[6],
        EXPECTED_FIELDS[6]: values[7],
    }


def assert_no_host_absolute_paths(value: Any, location: str = "oracle JSON") -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            assert_no_host_absolute_paths(child, f"{location}.{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            assert_no_host_absolute_paths(child, f"{location}[{index}]")
    elif isinstance(value, str):
        lowered = value.lower()
        if (
            value.startswith(("/", "\\\\"))
            or WINDOWS_ABSOLUTE_RE.match(value)
            or lowered.startswith("file:")
        ):
            raise ValueError(f"{location} contains a host absolute path: {value!r}")


def run_oracle(
    command_prefix: list[str],
    request: dict[str, Any],
) -> dict[str, Any]:
    result = subprocess.run(
        [*command_prefix, "-c", WORKER],
        input=json.dumps(request),
        text=True,
        encoding="utf-8",
        errors="replace",
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(
            "Vina Python API failed with exit "
            f"{result.returncode}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
    marker_lines = [
        line for line in result.stdout.splitlines() if line.startswith(RESULT_MARKER)
    ]
    if len(marker_lines) != 1:
        raise RuntimeError(
            "Vina Python API did not emit exactly one structured result\n"
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
    try:
        response = json.loads(marker_lines[0][len(RESULT_MARKER) :])
    except json.JSONDecodeError as error:
        raise RuntimeError(f"invalid Vina Python API result: {error}") from error
    if not isinstance(response, dict):
        raise RuntimeError("Vina Python API result is not an object")
    return response


def oracle_fixture_id(case_id: str) -> str:
    return f"oracle-{case_id}"


def upsert_oracle_fixture(
    manifest: dict[str, Any],
    case: dict[str, Any],
    destination: Path,
) -> None:
    path = case["oracle_output_path"]
    fixture_id = oracle_fixture_id(case["id"])
    entry = next(
        (
            value
            for value in manifest["fixtures"]
            if value["id"] == fixture_id or value["path"] == path
        ),
        None,
    )
    citation_keys = ["autodock-vina-1.2", "autodock-vina-original"]
    if case["scoring"] == "vinardo":
        citation_keys.append("vinardo")
    if case["id"].startswith("1iep-"):
        citation_keys.append("pdb-1iep")
    canonical = {
        "id": fixture_id,
        "path": path,
        "sha256": sha256(destination),
        "bytes": destination.stat().st_size,
        "kind": "oracle_json",
        "origin": "project_generated",
        "source": {
            "description": (
                "Generated by scripts/generate_docking_scoring_fixtures.py "
                "with the hash-authenticated AutoDock Vina 1.2.7 Python API."
            ),
            "url": None,
        },
        "license": "AGPL-3.0-only",
        "redistribution": "Redistributed with this project under its license.",
        "citation_keys": citation_keys,
        "expected": {
            "case_id": case["id"],
            "raw_vector_length": 8,
        },
    }
    if entry is None:
        manifest["fixtures"].append(canonical)
    else:
        entry.clear()
        entry.update(canonical)


def refresh_fixture_hashes(root: Path, manifest: dict[str, Any]) -> None:
    payloads: list[tuple[dict[str, Any], bytes]] = []
    for entry in manifest["fixtures"]:
        pure = safe_relative_path(entry["path"])
        path = root.joinpath(*pure.parts)
        if not path.is_file():
            raise ValueError(f"manifest-managed fixture is missing: {entry['path']}")
        payload = path.read_bytes()
        verify_official_git_blob(entry, payload)
        payloads.append((entry, payload))

    for entry, payload in payloads:
        entry["sha256"] = hashlib.sha256(payload).hexdigest()
        entry["bytes"] = len(payload)


def validate_manifest_oracle(parser: argparse.ArgumentParser, manifest: dict[str, Any]) -> None:
    oracle = manifest.get("oracle")
    if not isinstance(oracle, dict):
        parser.error("manifest oracle is missing")
    if (
        oracle.get("version") != EXPECTED_VINA_VERSION
        or oracle.get("git_commit") != EXPECTED_COMMIT
        or oracle.get("method") != EXPECTED_METHOD
        or oracle.get("numpy_version") != EXPECTED_NUMPY_VERSION
    ):
        parser.error("manifest oracle identity or Python API method is not pinned as expected")
    artifacts = oracle.get("wheel_artifacts")
    if not isinstance(artifacts, dict):
        parser.error("manifest oracle wheel_artifacts are missing")
    for package, expected in EXPECTED_WHEELS.items():
        actual = artifacts.get(package)
        if not isinstance(actual, dict):
            parser.error(f"manifest lacks {package} wheel artifact metadata")
        if any(actual.get(key) != expected[key] for key in ("filename", "sha256", "version")):
            parser.error(f"manifest {package} wheel artifact metadata differs from the pinned archive")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--vina-python-path",
        required=True,
        help="explicit Python interpreter containing Vina 1.2.7 and NumPy 1.26.4",
    )
    parser.add_argument("--vina-wheel-path", type=Path, required=True)
    parser.add_argument("--vina-wheel-sha256", required=True)
    parser.add_argument("--numpy-wheel-path", type=Path, required=True)
    parser.add_argument("--numpy-wheel-sha256", required=True)
    parser.add_argument(
        "--fixtures-dir",
        type=Path,
        default=Path(__file__).resolve().parent / "fixtures" / "docking",
    )
    parser.add_argument(
        "--case",
        action="append",
        dest="cases",
        help="generate only this scoring-case id (repeatable)",
    )
    parser.add_argument(
        "--write-manifest",
        action="store_true",
        help="set generated status, expected values, and fixture hashes in manifest.json",
    )
    args = parser.parse_args()

    vina_wheel = args.vina_wheel_path.expanduser().resolve()
    numpy_wheel = args.numpy_wheel_path.expanduser().resolve()
    packages = {
        "vina": verify_wheel(
            parser, "vina", vina_wheel, args.vina_wheel_sha256
        ),
        "numpy": verify_wheel(
            parser, "numpy", numpy_wheel, args.numpy_wheel_sha256
        ),
    }
    command_prefix, translate_path = local_python_launcher(
        parser, args.vina_python_path
    )

    root = args.fixtures_dir.expanduser().resolve()
    manifest_path = root / "manifest.json"
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        parser.error(f"cannot load fixture manifest: {error}")
    validate_manifest_oracle(parser, manifest)

    selected = set(args.cases or [])
    known = {case["id"] for case in manifest["scoring_cases"]}
    unknown = selected - known
    if unknown:
        parser.error(f"unknown scoring cases: {', '.join(sorted(unknown))}")

    try:
        prepared_cases = preflight_selected_cases(root, manifest, selected)
    except ValueError as error:
        parser.error(str(error))

    generated: list[dict[str, Any]] = []
    for prepared in prepared_cases:
        case = prepared["case"]
        receptor_entry = prepared["receptor_entry"]
        ligand_entry = prepared["ligand_entry"]
        receptor = prepared["receptor"]
        ligand = prepared["ligand"]
        request = {
            "receptor_path": translate_path(receptor),
            "ligand_path": translate_path(ligand),
            "scoring": case["scoring"],
            "box_center": case["box_center"],
            "box_size": case["box_size"],
            "spacing": case["spacing"],
            "seed": case["seed"],
            "cpu": 1,
            "no_refine": False,
            "force_even_voxels": False,
            "packages": packages,
        }
        response = run_oracle(command_prefix, request)
        raw = [clean_energy(value) for value in response["raw_vector_kcal_mol"]]
        expected = map_expected(raw)
        output = {
            "$schema": "../manifest.schema.json#/$defs/oracleOutput",
            "schema_version": 1,
            "case_id": case["id"],
            "oracle": {
                "name": "AutoDock Vina",
                "version": EXPECTED_VINA_VERSION,
                "source_commit": EXPECTED_COMMIT,
                "method": EXPECTED_METHOD,
            },
            "scoring": case["scoring"],
            "configuration": {
                "box_center": case["box_center"],
                "box_size": case["box_size"],
                "spacing": case["spacing"],
                "seed": case["seed"],
                "cpu": 1,
                "no_refine": False,
                "force_even_voxels": False,
            },
            "weights": response["weights"],
            "packages": response["packages"],
            "runtime": response["runtime"],
            "fixtures": {
                "receptor": {
                    "id": case["receptor_fixture"],
                    "sha256": receptor_entry["sha256"],
                },
                "ligand": {
                    "id": case["ligand_fixture"],
                    "sha256": ligand_entry["sha256"],
                },
            },
            "api_result": {
                "raw_vector_columns": RAW_VECTOR_COLUMNS,
                "raw_vector_kcal_mol": raw,
            },
            "expected_mapping": EXPECTED_MAPPING,
            "expected": expected,
        }
        assert_no_host_absolute_paths(output)
        generated.append(
            {
                "case": case,
                "destination": prepared["destination"],
                "relative": prepared["output_relative"],
                "expected": expected,
                "serialized": json.dumps(output, indent=2) + "\n",
            }
        )

    for result in generated:
        destination = result["destination"]
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(
            result["serialized"],
            encoding="utf-8",
            newline="\n",
        )
        case = result["case"]
        if args.write_manifest:
            case["oracle_status"] = "generated"
            case["expected"] = result["expected"]
        print(f"generated {case['id']} -> {result['relative'].as_posix()}")

    if args.write_manifest:
        refresh_fixture_hashes(root, manifest)
        for result in generated:
            upsert_oracle_fixture(manifest, result["case"], result["destination"])
        manifest_path.write_text(
            json.dumps(manifest, indent=2) + "\n",
            encoding="utf-8",
            newline="\n",
        )
    print(
        f"generated {len(generated)} oracle fixture(s) with {EXPECTED_METHOD}; "
        "no package was downloaded or installed"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
