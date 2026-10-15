#!/usr/bin/env python3
"""Strict offline verifier for the docking fixture manifest and oracles."""

from __future__ import annotations

import hashlib
import json
import math
import re
import sys
from pathlib import Path, PurePosixPath
from typing import Any

ROOT = Path(__file__).resolve().parent
MANIFEST_PATH = ROOT / "manifest.json"
SCHEMA_PATH = ROOT / "manifest.schema.json"
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
WINDOWS_ABSOLUTE_RE = re.compile(r"^[A-Za-z]:[\\/]")
COMMIT = "8eb40404f4f45608acb3b01427587ac049f27c1f"
ORACLE_METHOD = "vina.Vina.score()"
VINA_VERSION = "1.2.7"
NUMPY_VERSION = "1.26.4"
SPACING = 0.375
SEED = 1
WHEEL_ARTIFACTS = {
    "vina": {
        "filename": "vina-1.2.7-cp312-cp312-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
        "version": VINA_VERSION,
        "sha256": "a6e46560e50e693fb29883b82be8d9a4d74207b4464cfedf2bd5235192f4eb3d",
        "digest_source": "PyPI published file digest",
    },
    "numpy": {
        "filename": "numpy-1.26.4-cp312-cp312-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
        "version": NUMPY_VERSION,
        "sha256": "675d61ffbfa78604709862923189bad94014bef562cc35cf61d3a07bba02a7ed",
        "digest_source": "PyPI published file digest",
    },
}
WEIGHTS = {
    "vina": [-0.035579, -0.005156, 0.840245, -0.035069, -0.587439, 50.0, 0.05846],
    "vinardo": [-0.045, 0.8, -0.035, -0.6, 50.0, 0.05846],
}
RAW_COLUMNS = [
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
EXPECTED_FIXTURE_FIELDS = {
    "license-autodock-vina": {"title"},
    "license-pdb-cc0": {"title"},
    "manifest-schema": {"json_schema_draft"},
    "vina-1iep-ligand": {"atom_records", "branches", "torsdof"},
    "vina-1iep-receptor": {"atom_records", "branches"},
    "vina-1iep-receptor-pdb": {"atom_records", "parser_only"},
    "branch-order-nested": {"atom_serial_order", "branch_parents", "torsdof"},
    "h-hd-aliases": {
        "atom_type_tokens",
        "H_is_nonpolar_and_only_HD_confers_donor_identity",
    },
    "malformed-unclosed-branch": {"parse_error_contains"},
    "clash-receptor": {"atom_records", "coincident_pair_position_angstrom"},
    "clash-ligand": {"atom_records", "torsdof", "coincident_pair_position_angstrom"},
    "carbon-contact-receptor": {"atom_records"},
    "carbon-contact-ligand": {"atom_records", "torsdof", "pair_distance_angstrom"},
    "hbond-hd-receptor": {"atom_records", "contains_atom_type"},
    "hbond-acceptor-ligand": {
        "atom_records",
        "torsdof",
        "donor_acceptor_distance_angstrom",
    },
    "provenance-md": {"document"},
    "verify-py": {"role"},
    "verify-manifest-test-py": {"role"},
}
EXPECTED_CASES = {
    "clash-vina": ("clash-receptor", "clash-ligand", "vina"),
    "clash-vinardo": ("clash-receptor", "clash-ligand", "vinardo"),
    "carbon-contact-vina": ("carbon-contact-receptor", "carbon-contact-ligand", "vina"),
    "carbon-contact-vinardo": ("carbon-contact-receptor", "carbon-contact-ligand", "vinardo"),
    "hbond-hd-vina": ("hbond-hd-receptor", "hbond-acceptor-ligand", "vina"),
    "hbond-hd-vinardo": ("hbond-hd-receptor", "hbond-acceptor-ligand", "vinardo"),
    "1iep-vina": ("vina-1iep-receptor", "vina-1iep-ligand", "vina"),
    "1iep-vinardo": ("vina-1iep-receptor", "vina-1iep-ligand", "vinardo"),
}
FIXTURE_FIELDS = {
    "id",
    "path",
    "sha256",
    "bytes",
    "kind",
    "origin",
    "source",
    "license",
    "redistribution",
    "citation_keys",
    "expected",
}
CASE_FIELDS = {
    "id",
    "receptor_fixture",
    "ligand_fixture",
    "scoring",
    "box_center",
    "box_size",
    "spacing",
    "seed",
    "oracle_method",
    "oracle_version",
    "oracle_status",
    "oracle_output_path",
    "expected",
}
ORACLE_OUTPUT_FIELDS = {
    "$schema",
    "schema_version",
    "case_id",
    "oracle",
    "scoring",
    "configuration",
    "weights",
    "packages",
    "runtime",
    "fixtures",
    "api_result",
    "expected_mapping",
    "expected",
}


class VerificationError(ValueError):
    pass


def fail(message: str) -> None:
    raise VerificationError(message)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"cannot read {path.name}: {error}")
    require(isinstance(value, dict), f"{path.name} must contain a JSON object")
    return value


def git_blob_oid(payload: bytes) -> str:
    header = f"blob {len(payload)}\0".encode("ascii")
    return hashlib.sha1(header + payload).hexdigest()


def safe_path(relative: Any) -> Path:
    require(isinstance(relative, str) and relative, "fixture path must be a string")
    require("\\" not in relative, f"fixture path must use '/': {relative!r}")
    pure = PurePosixPath(relative)
    require(not pure.is_absolute(), f"fixture path is absolute: {relative!r}")
    require(".." not in pure.parts, f"fixture path escapes root: {relative!r}")
    return ROOT.joinpath(*pure.parts)


def assert_no_host_absolute_paths(value: Any, location: str) -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            assert_no_host_absolute_paths(child, f"{location}.{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            assert_no_host_absolute_paths(child, f"{location}[{index}]")
    elif isinstance(value, str):
        lowered = value.lower()
        require(
            not (
                value.startswith(("/", "\\\\"))
                or WINDOWS_ABSOLUTE_RE.match(value)
                or lowered.startswith("file:")
            ),
            f"{location} contains a host absolute path: {value!r}",
        )


def atom_lines(path: Path) -> list[str]:
    return [
        line
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.startswith(("ATOM  ", "HETATM"))
    ]


def nested_branch_parents(path: Path) -> list[int | None]:
    parents: list[int | None] = []
    stack: list[int] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("BRANCH"):
            parents.append(stack[-1] if stack else None)
            stack.append(len(parents) - 1)
        elif line.startswith("ENDBRANCH"):
            require(bool(stack), f"{path.name}: ENDBRANCH without BRANCH")
            stack.pop()
    require(not stack, f"{path.name}: branch stack remains open")
    return parents


def require_branch_bodies(path: Path) -> None:
    branches: list[dict[str, Any]] = []
    stack: list[int] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("BRANCH"):
            fields = line.split()
            require(len(fields) == 3, f"{path.name}: malformed BRANCH record")
            branches.append({"child": int(fields[2]), "direct_serials": []})
            stack.append(len(branches) - 1)
        elif line.startswith(("ATOM  ", "HETATM")) and stack:
            branches[stack[-1]]["direct_serials"].append(int(line[6:11]))
        elif line.startswith("ENDBRANCH"):
            require(bool(stack), f"{path.name}: ENDBRANCH without BRANCH")
            stack.pop()
    for index, branch in enumerate(branches):
        require(
            any(serial != branch["child"] for serial in branch["direct_serials"]),
            f"{path.name}: branch {index} contains only its child-axis atom",
        )


def check_expected(entry: dict[str, Any], path: Path) -> None:
    expected = entry["expected"]
    fixture_id = entry["id"]
    require(isinstance(expected, dict), f"{fixture_id}: expected must be an object")
    if entry["kind"] == "oracle_json":
        required_expected = {"case_id", "raw_vector_length"}
    else:
        required_expected = EXPECTED_FIXTURE_FIELDS.get(fixture_id)
        require(required_expected is not None, f"{fixture_id}: no expected-field contract")
    require(set(expected) == required_expected, f"{fixture_id}: expected metadata fields differ")
    if "atom_records" in expected:
        require(
            len(atom_lines(path)) == expected["atom_records"],
            f"{entry['id']}: atom record count differs",
        )
    if "branches" in expected:
        branch_count = sum(
            line.startswith("BRANCH")
            for line in path.read_text(encoding="utf-8").splitlines()
        )
        require(branch_count == expected["branches"], f"{entry['id']}: branch count differs")
    if "torsdof" in expected:
        torsdof = [
            line.split()[1]
            for line in path.read_text(encoding="utf-8").splitlines()
            if line.startswith("TORSDOF ")
        ]
        require(torsdof == [str(expected["torsdof"])], f"{entry['id']}: TORSDOF differs")
    if "atom_serial_order" in expected:
        serials = [int(line[6:11]) for line in atom_lines(path)]
        require(serials == expected["atom_serial_order"], f"{entry['id']}: serial order differs")
    if "branch_parents" in expected:
        require(
            nested_branch_parents(path) == expected["branch_parents"],
            f"{entry['id']}: branch parent topology differs",
        )
        require_branch_bodies(path)
    if "atom_type_tokens" in expected:
        tokens = [line.split()[-1] for line in atom_lines(path)]
        require(tokens == expected["atom_type_tokens"], f"{entry['id']}: atom types differ")
    if "contains_atom_type" in expected:
        tokens = [line.split()[-1] for line in atom_lines(path)]
        require(expected["contains_atom_type"] in tokens, f"{entry['id']}: atom type missing")
    if "parse_error_contains" in expected:
        text = path.read_text(encoding="utf-8")
        depth = 0
        for line in text.splitlines():
            if line.startswith("BRANCH"):
                depth += 1
            elif line.startswith("ENDBRANCH"):
                depth -= 1
        require(depth > 0, f"{entry['id']}: malformed branch fixture is no longer malformed")
    if "coincident_pair_position_angstrom" in expected:
        atoms = atom_lines(path)
        require(len(atoms) == 1, f"{entry['id']}: clash fixture must have one atom")
        position = [
            float(atoms[0][30:38]),
            float(atoms[0][38:46]),
            float(atoms[0][46:54]),
        ]
        require(
            position == expected["coincident_pair_position_angstrom"] == [0, 0, 0],
            f"{entry['id']}: clash coordinate metadata differs",
        )
    if "pair_distance_angstrom" in expected:
        atoms = atom_lines(path)
        require(len(atoms) == 1, f"{entry['id']}: pair-distance ligand must have one atom")
        x = float(atoms[0][30:38])
        require(x == expected["pair_distance_angstrom"], f"{entry['id']}: pair distance metadata differs")
    if "donor_acceptor_distance_angstrom" in expected:
        atoms = atom_lines(path)
        require(len(atoms) == 1, f"{entry['id']}: donor-acceptor ligand must have one atom")
        x = float(atoms[0][30:38])
        require(x == expected["donor_acceptor_distance_angstrom"], f"{entry['id']}: donor-acceptor distance metadata differs")
    if "parser_only" in expected:
        require(expected["parser_only"] is True, f"{entry['id']}: parser_only must be true")
    if "H_is_nonpolar_and_only_HD_confers_donor_identity" in expected:
        require(
            expected["H_is_nonpolar_and_only_HD_confers_donor_identity"] is True,
            f"{entry['id']}: hydrogen identity assertion must be true",
        )
    if "title" in expected:
        text = path.read_text(encoding="utf-8")
        normalized_text = " ".join(text.split())
        title_tokens = expected["title"].split()
        cursor = 0
        for token in title_tokens:
            cursor = normalized_text.find(token, cursor)
            require(cursor >= 0, f"{entry['id']}: expected title is absent")
            cursor += len(token)
    if "json_schema_draft" in expected:
        value = load_json(path)
        require(
            value.get("$schema") == "https://json-schema.org/draft/2020-12/schema",
            f"{entry['id']}: JSON Schema draft differs",
        )
    if "document" in expected:
        require(expected["document"] == "docking fixture provenance", f"{entry['id']}: document role differs")
        require(path.read_text(encoding="utf-8").startswith("# Docking fixture provenance"), f"{entry['id']}: provenance heading differs")
    if "role" in expected:
        require(isinstance(expected["role"], str) and expected["role"], f"{entry['id']}: role is empty")
    if "case_id" in expected:
        value = load_json(path)
        require(value.get("case_id") == expected["case_id"], f"{entry['id']}: case id differs")
        raw = value.get("api_result", {}).get("raw_vector_kcal_mol")
        require(
            isinstance(raw, list) and len(raw) == expected.get("raw_vector_length"),
            f"{entry['id']}: raw vector length differs",
        )


def clean_energy(value: float) -> float:
    rounded = round(float(value), 3)
    return 0.0 if rounded == 0 else rounded


def expected_from_raw(raw: list[float]) -> dict[str, float]:
    values = [clean_energy(value) for value in raw]
    require(len(values) == 8, "raw score vector must contain eight values")
    return {
        EXPECTED_FIELDS[0]: values[0],
        EXPECTED_FIELDS[1]: clean_energy(values[1] + values[2]),
        EXPECTED_FIELDS[2]: values[1],
        EXPECTED_FIELDS[3]: clean_energy(values[3] + values[4] + values[5]),
        EXPECTED_FIELDS[4]: values[5],
        EXPECTED_FIELDS[5]: values[6],
        EXPECTED_FIELDS[6]: values[7],
    }


def require_number(value: Any, message: str) -> float:
    require(isinstance(value, (int, float)) and not isinstance(value, bool), message)
    numeric = float(value)
    require(math.isfinite(numeric), message)
    return numeric


def require_vector3(value: Any, message: str, positive: bool = False) -> list[float]:
    require(isinstance(value, list) and len(value) == 3, message)
    vector = [require_number(item, message) for item in value]
    if positive:
        require(all(item > 0 for item in vector), message)
    return vector


def check_package_record(
    record: Any,
    package: str,
    case_id: str,
) -> None:
    expected = WHEEL_ARTIFACTS[package]
    require(isinstance(record, dict), f"{case_id}: {package} package record missing")
    require(
        set(record)
        == {
            "distribution_version",
            "module_version",
            "wheel_filename",
            "wheel_sha256",
            "installation_provenance",
        },
        f"{case_id}: {package} package record fields differ",
    )
    require(record["distribution_version"] == expected["version"], f"{case_id}: {package} distribution version differs")
    require(record["module_version"] == expected["version"], f"{case_id}: {package} module version differs")
    require(record["wheel_filename"] == expected["filename"], f"{case_id}: {package} wheel filename differs")
    require(record["wheel_sha256"] == expected["sha256"], f"{case_id}: {package} wheel digest differs")
    provenance = record["installation_provenance"]
    require(isinstance(provenance, dict), f"{case_id}: {package} installation provenance missing")
    require(
        provenance
        == {
            "method": "direct_url_archive_hash",
            "wheel_filename": expected["filename"],
            "sha256": expected["sha256"],
        },
        f"{case_id}: {package} installed distribution provenance differs",
    )


def check_oracle_output(
    case: dict[str, Any],
    output_path: Path,
    by_id: dict[str, dict[str, Any]],
) -> None:
    output = load_json(output_path)
    case_id = case["id"]
    require(set(output) == ORACLE_OUTPUT_FIELDS, f"{case_id}: oracle output fields differ")
    assert_no_host_absolute_paths(output, case_id)
    require(output["$schema"] == "../manifest.schema.json#/$defs/oracleOutput", f"{case_id}: oracle schema reference differs")
    require(output["schema_version"] == 1, f"{case_id}: oracle schema version differs")
    require(output["case_id"] == case_id, f"{case_id}: oracle case id differs")
    require(output["scoring"] == case["scoring"], f"{case_id}: oracle scoring differs")
    require(
        output["oracle"]
        == {
            "name": "AutoDock Vina",
            "version": VINA_VERSION,
            "source_commit": COMMIT,
            "method": ORACLE_METHOD,
        },
        f"{case_id}: oracle identity differs",
    )

    configuration = output["configuration"]
    require(
        isinstance(configuration, dict)
        and set(configuration)
        == {
            "box_center",
            "box_size",
            "spacing",
            "seed",
            "cpu",
            "no_refine",
            "force_even_voxels",
        },
        f"{case_id}: configuration fields differ",
    )
    require(configuration["box_center"] == case["box_center"], f"{case_id}: box center differs")
    require(configuration["box_size"] == case["box_size"], f"{case_id}: box size differs")
    require(configuration["spacing"] == SPACING, f"{case_id}: spacing differs")
    require(configuration["seed"] == SEED, f"{case_id}: seed differs")
    require(configuration["cpu"] == 1, f"{case_id}: CPU setting differs")
    require(configuration["no_refine"] is False, f"{case_id}: no_refine must be false")
    require(configuration["force_even_voxels"] is False, f"{case_id}: force_even_voxels must be false")

    require(output["weights"] == WEIGHTS[case["scoring"]], f"{case_id}: scoring weights differ")
    packages = output["packages"]
    require(isinstance(packages, dict) and set(packages) == {"vina", "numpy"}, f"{case_id}: package fields differ")
    check_package_record(packages["vina"], "vina", case_id)
    check_package_record(packages["numpy"], "numpy", case_id)

    runtime = output["runtime"]
    require(isinstance(runtime, dict) and set(runtime) == {"python_implementation", "python_version"}, f"{case_id}: runtime fields differ")
    require(runtime["python_implementation"] == "CPython", f"{case_id}: Python implementation differs")
    require(isinstance(runtime["python_version"], str) and re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", runtime["python_version"]), f"{case_id}: Python version is invalid")

    fixtures = output["fixtures"]
    require(isinstance(fixtures, dict) and set(fixtures) == {"receptor", "ligand"}, f"{case_id}: fixture identities differ")
    for role, case_key in (("receptor", "receptor_fixture"), ("ligand", "ligand_fixture")):
        fixture_id = case[case_key]
        require(
            fixtures[role]
            == {"id": fixture_id, "sha256": by_id[fixture_id]["sha256"]},
            f"{case_id}: {role} fixture identity differs",
        )

    api_result = output["api_result"]
    require(isinstance(api_result, dict) and set(api_result) == {"raw_vector_columns", "raw_vector_kcal_mol"}, f"{case_id}: API result fields differ")
    require(api_result["raw_vector_columns"] == RAW_COLUMNS, f"{case_id}: raw columns differ")
    raw = api_result["raw_vector_kcal_mol"]
    require(isinstance(raw, list) and len(raw) == 8, f"{case_id}: raw vector must have eight columns")
    raw_numbers = [require_number(value, f"{case_id}: raw vector contains a non-finite number") for value in raw]
    require(raw_numbers == [clean_energy(value) for value in raw_numbers], f"{case_id}: raw vector is not normalized to three decimals")
    require(output["expected_mapping"] == EXPECTED_MAPPING, f"{case_id}: expected mapping semantics differ")
    mapped = expected_from_raw(raw_numbers)
    require(output["expected"] == mapped, f"{case_id}: oracle expected mapping differs from raw vector")
    require(case["expected"] == mapped, f"{case_id}: manifest expected mapping differs from raw vector")


def verify() -> tuple[int, int]:
    manifest = load_json(MANIFEST_PATH)
    schema = load_json(SCHEMA_PATH)
    require(schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema", "schema draft differs")
    require(manifest.get("$schema") == "./manifest.schema.json", "manifest schema reference differs")
    require(manifest.get("schema_version") == 1, "schema_version must be 1")

    oracle = manifest.get("oracle")
    require(isinstance(oracle, dict), "oracle must be an object")
    require(
        set(oracle)
        == {
            "name",
            "version",
            "git_commit",
            "tag_url",
            "source_url",
            "license",
            "binary_policy",
            "method",
            "numpy_version",
            "wheel_artifacts",
        },
        "oracle fields differ",
    )
    require(oracle["name"] == "AutoDock Vina", "oracle name differs")
    require(oracle["version"] == VINA_VERSION, "oracle version differs")
    require(oracle["git_commit"] == COMMIT, "oracle commit differs")
    require(oracle["method"] == ORACLE_METHOD, "oracle method differs")
    require(oracle["numpy_version"] == NUMPY_VERSION, "oracle NumPy version differs")
    require(oracle["wheel_artifacts"] == WHEEL_ARTIFACTS, "oracle wheel artifacts differ")
    require(oracle["license"] == "Apache-2.0", "oracle license differs")
    require("does not distribute or download" in oracle["binary_policy"], "oracle package policy differs")
    assert_no_host_absolute_paths(oracle, "manifest.oracle")

    tolerances = manifest.get("tolerances")
    require(isinstance(tolerances, dict), "tolerances must be an object")
    require(set(tolerances) == {"vina", "vinardo"}, "separate vina/vinardo tolerances required")
    for scoring, values in tolerances.items():
        require(isinstance(values, dict), f"{scoring} tolerance must be an object")
        require(set(values) == {"affinity_kcal_mol_abs", "component_kcal_mol_abs"}, f"{scoring} tolerance fields differ")
        require(all(isinstance(value, (int, float)) and value > 0 for value in values.values()), f"{scoring} tolerances must be positive")

    citations = manifest.get("citations")
    require(isinstance(citations, dict) and citations, "citations must be a non-empty object")

    fixtures = manifest.get("fixtures")
    require(isinstance(fixtures, list) and fixtures, "fixtures must be a non-empty array")
    ids: set[str] = set()
    paths: set[str] = set()
    by_id: dict[str, dict[str, Any]] = {}
    for entry in fixtures:
        require(isinstance(entry, dict), "fixture entry must be an object")
        require(set(entry) == FIXTURE_FIELDS, f"fixture {entry.get('id')!r}: fields differ")
        fixture_id = entry["id"]
        require(isinstance(fixture_id, str) and fixture_id not in ids, f"duplicate fixture id: {fixture_id!r}")
        ids.add(fixture_id)
        by_id[fixture_id] = entry
        relative = entry["path"]
        require(relative not in paths, f"duplicate fixture path: {relative!r}")
        paths.add(relative)
        path = safe_path(relative)
        require(path.is_file(), f"missing fixture: {relative}")
        payload = path.read_bytes()
        digest = hashlib.sha256(payload).hexdigest()
        require(isinstance(entry["sha256"], str) and SHA256_RE.fullmatch(entry["sha256"]) is not None, f"{fixture_id}: invalid SHA256")
        require(digest == entry["sha256"], f"{fixture_id}: SHA256 mismatch")
        require(len(payload) == entry["bytes"], f"{fixture_id}: byte length mismatch")
        require(entry["origin"] in {"official_upstream", "project_authored", "project_generated"}, f"{fixture_id}: bad origin")
        if entry["kind"] == "oracle_json":
            require(entry["origin"] == "project_generated", f"{fixture_id}: oracle JSON must be project_generated")
        else:
            require(entry["origin"] != "project_generated", f"{fixture_id}: only oracle JSON may be project_generated")
        source = entry["source"]
        require(isinstance(source, dict) and source.get("description"), f"{fixture_id}: source missing")
        if entry["origin"] == "official_upstream" and "git_commit" in source:
            require(source["git_commit"] == COMMIT, f"{fixture_id}: source commit differs")
        if entry["origin"] == "official_upstream" and "git_blob" in source:
            recorded_blob = source["git_blob"]
            require(
                isinstance(recorded_blob, str) and re.fullmatch(r"[0-9a-f]{40}", recorded_blob) is not None,
                f"{fixture_id}: invalid recorded Git blob OID",
            )
            require(
                git_blob_oid(payload) == recorded_blob,
                f"{fixture_id}: Git blob OID mismatch",
            )
        require(isinstance(entry["redistribution"], str) and entry["redistribution"], f"{fixture_id}: redistribution missing")
        require(isinstance(entry["citation_keys"], list) and len(set(entry["citation_keys"])) == len(entry["citation_keys"]), f"{fixture_id}: citation keys must be unique")
        require(all(key in citations for key in entry["citation_keys"]), f"{fixture_id}: unknown citation key")
        check_expected(entry, path)

    cases = manifest.get("scoring_cases")
    require(isinstance(cases, list) and cases, "scoring_cases must be a non-empty array")
    require(len(cases) == len(EXPECTED_CASES), "scoring case count differs")
    case_ids: set[str] = set()
    oracle_paths: set[str] = set()
    for case in cases:
        require(isinstance(case, dict), "scoring case must be an object")
        require(set(case) == CASE_FIELDS, f"scoring case {case.get('id')!r}: fields differ")
        case_id = case["id"]
        require(isinstance(case_id, str) and case_id not in case_ids, f"duplicate scoring case: {case_id!r}")
        case_ids.add(case_id)
        require(case_id in EXPECTED_CASES, f"{case_id}: unknown scoring case")
        expected_receptor, expected_ligand, expected_scoring = EXPECTED_CASES[case_id]
        require(case["receptor_fixture"] == expected_receptor, f"{case_id}: receptor fixture differs")
        require(case["ligand_fixture"] == expected_ligand, f"{case_id}: ligand fixture differs")
        require(case["scoring"] == expected_scoring, f"{case_id}: scoring mode differs")
        require(case["receptor_fixture"] in by_id, f"{case_id}: unknown receptor")
        require(case["ligand_fixture"] in by_id, f"{case_id}: unknown ligand")
        require(by_id[case["receptor_fixture"]]["kind"] == "receptor_pdbqt", f"{case_id}: receptor fixture kind differs")
        require(by_id[case["ligand_fixture"]]["kind"] == "ligand_pdbqt", f"{case_id}: ligand fixture kind differs")
        require(case["scoring"] in tolerances, f"{case_id}: unknown scoring mode")
        require_vector3(case["box_center"], f"{case_id}: invalid box center")
        require_vector3(case["box_size"], f"{case_id}: invalid box size", positive=True)
        if case_id.startswith("clash-"):
            require(case["box_center"] == [0, 0, 0], f"{case_id}: clash box center differs")
            require(case["box_size"] == [8, 8, 8], f"{case_id}: micro box size differs")
        elif case_id.startswith("carbon-contact-"):
            require(case["box_center"] == [3.8, 0, 0], f"{case_id}: carbon-contact box center differs")
            require(case["box_size"] == [8, 8, 8], f"{case_id}: micro box size differs")
        elif case_id.startswith("hbond-hd-"):
            require(case["box_center"] == [3, 0, 0], f"{case_id}: hbond box center differs")
            require(case["box_size"] == [8, 8, 8], f"{case_id}: micro box size differs")
        elif case_id.startswith("1iep-"):
            require(case["box_center"] == [15.19, 53.9025, 16.917], f"{case_id}: 1IEP box center differs")
            require(case["box_size"] == [20, 28, 24], f"{case_id}: 1IEP box size differs")
        else:
            fail(f"{case_id}: scoring case has no pinned box geometry")
        require(case["spacing"] == SPACING, f"{case_id}: spacing must be {SPACING}")
        require(case["seed"] == SEED, f"{case_id}: seed must be {SEED}")
        require(case["oracle_method"] == ORACLE_METHOD, f"{case_id}: oracle method differs")
        require(case["oracle_version"] == VINA_VERSION, f"{case_id}: oracle version differs")
        output_relative = case["oracle_output_path"]
        require(output_relative not in oracle_paths, f"{case_id}: duplicate oracle output path")
        oracle_paths.add(output_relative)
        output_path = safe_path(output_relative)
        fixture_id = f"oracle-{case_id}"
        if case["oracle_status"] == "not_generated":
            require(case["expected"] is None, f"{case_id}: ungenerated oracle must have null expected")
            require(not output_path.exists(), f"{case_id}: output exists but status is not_generated")
            require(fixture_id not in by_id, f"{case_id}: ungenerated oracle is listed as a fixture")
        elif case["oracle_status"] == "generated":
            require(isinstance(case["expected"], dict) and set(case["expected"]) == set(EXPECTED_FIELDS), f"{case_id}: generated expected fields differ")
            require(output_path.is_file(), f"{case_id}: generated oracle output missing")
            require(fixture_id in by_id, f"{case_id}: generated oracle is not manifest-managed")
            require(by_id[fixture_id]["path"] == output_relative, f"{case_id}: oracle fixture path differs")
            check_oracle_output(case, output_path, by_id)
            if case_id.startswith("clash-"):
                require(
                    case["expected"]["estimated_free_energy_of_binding_kcal_mol"] > 0,
                    f"{case_id}: clash affinity must exercise positive soft-curl",
                )
                require(
                    case["expected"]["final_intermolecular_energy_kcal_mol"] > 0,
                    f"{case_id}: clash intermolecular energy must be positive",
                )
        else:
            fail(f"{case_id}: invalid oracle_status")

    generated_fixture_ids = {
        entry["id"] for entry in fixtures if entry["kind"] == "oracle_json"
    }
    require(
        generated_fixture_ids
        == {
            f"oracle-{case['id']}"
            for case in cases
            if case["oracle_status"] == "generated"
        },
        "oracle fixture inventory differs from generated scoring cases",
    )

    managed = paths | {"manifest.json"}
    actual = {
        path.relative_to(ROOT).as_posix()
        for path in ROOT.rglob("*")
        if path.is_file() and "__pycache__" not in path.parts
    }
    require(actual == managed, f"unmanifested or missing files: extra={sorted(actual-managed)}, missing={sorted(managed-actual)}")
    return len(fixtures), len(cases)


def main() -> int:
    try:
        fixture_count, case_count = verify()
    except VerificationError as error:
        print(f"docking fixtures: FAIL: {error}", file=sys.stderr)
        return 1
    print(
        "docking fixtures: OK "
        f"({fixture_count} files, {case_count} scoring cases; "
        "all checksums and oracle provenance verified offline)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
