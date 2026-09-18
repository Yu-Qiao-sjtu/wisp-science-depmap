"""Read-only, bounded DepMap 26Q1 knowledge API.

The service validates a small query contract and delegates scientific data
access to the already-tested R knowledge query helper. It never exposes raw
matrices or starts new analyses.
"""

from __future__ import annotations

import asyncio
import gzip
import hmac
import json
import logging
import os
import csv
import re
import sqlite3
from contextlib import asynccontextmanager, closing
from dataclasses import dataclass
from pathlib import Path
from typing import Annotated, Any, Awaitable, Callable, Literal

from fastapi import Depends, FastAPI, Header, HTTPException, Request, status
from fastapi.responses import JSONResponse
from pydantic import BaseModel, ConfigDict, Field, model_validator

from fastapi.exceptions import RequestValidationError

from services.depmap_api.provider_schema import MODE_LIMIT_MAX, schema_violation
from services.depmap_api.scientific_query import (
    EVIDENCE_STATUSES,
    bound_after_rank,
    classify_coverage,
    classify_exact_entity,
    classify_tested_entity,
    criteria_failures,
    filter_before_limit,
    annotate_common_essential,
)


LOGGER = logging.getLogger("depmap_api")
MAX_REQUEST_BYTES = 8 * 1024
MAX_RESPONSE_BYTES = 4 * 1024 * 1024
MAX_STDERR_BYTES = 8 * 1024
MATRIX_MODULES = {
    "effect_correlation",
    "expression_correlation",
    "expression_dependency",
    "damaging_mutation_dependency",
    "custom_missense_mutation_dependency",
    "hotspot_mutation_dependency",
    "cnv_amplification_dependency",
}
LINEAGE_EVENTS = {"damaging", "custom_missense", "hotspot"}
DRUG_OMICS = {"effect", "expression", "cnv"}
SYNTHETIC_LETHAL_EVENTS = {
    "damaging_mutation", "custom_missense_mutation", "hotspot_mutation", "cnv_amplification"
}
THREE_D_FAMILIES = {
    "dependency_profiles", "differential_dependency", "codependency",
    "true_love_gene", "omics_dependency", "lineage_dependency_enrichment",
}
THREE_D_OMICS = {"expression", "cnv", "damaging", "hotspot"}
QUERY_CONTRACT_VERSION = 10
MODE_REQUIRED_FIELDS = {
    "analysis_catalog": set(),
    "mutation_anchor": {"lineage"},
    "lineage_mutation_dependency": {"lineage"},
    "catalog": set(),
    "lineage_catalog": {"lineage"},
    "lineage_dependency": {"lineage"},
    "pan_cancer_dependency": set(),
    "lineage_directions": {"lineage"},
    "core": {"gene"},
    "pair": {"module", "source", "target"},
    "top": {"module", "source", "limit"},
    "lineage": {"event", "lineage", "source", "target"},
    "pathway": {"pathway", "target"},
    "drug": {"drug", "target", "omic"},
    "lineage_network": {"family", "lineage", "source"},
    "lineage_cnv": {"lineage", "source"},
    "lineage_drug": {"omic", "lineage"},
    "enrichment": {"lineage", "source"},
    "subtype": set(),
    "coamplification": {"source"},
    "true_love": set(),
    "synthetic_lethal": set(),
    "three_d": {"family"},
    "tcga_expression_survival": {"gene"},
    "tf_dependency": set(),
    "biomarker_target": {"target"},
}
MODE_OPTIONAL_FIELDS = {
    "analysis_catalog": {"module", "completion_state", "limit"},
    "mutation_anchor": {"gene", "event", "anchor_tier", "include_common_essential", "limit"},
    "lineage_mutation_dependency": {"source", "target", "event", "limit"},
    "lineage_network": {"target", "limit", "reciprocal"},
    "lineage_dependency": {"gene", "ranking", "exclude_common_essential", "common_essential_source", "limit"},
    "pan_cancer_dependency": {"gene", "ranking", "exclude_common_essential", "common_essential_source", "limit"},
    "lineage_directions": {"limit"},
    "lineage_cnv": {"target", "limit"},
    "lineage_drug": {"drug", "target", "limit"},
    "enrichment": {"collection", "term", "limit"},
    "subtype": {"gene", "lineage", "contrast", "limit"},
    "coamplification": {"partner", "target", "layer", "limit"},
    "true_love": {"gene", "partner", "catalog", "coverage", "limit"},
    "synthetic_lethal": {"source", "target", "event", "lineage", "limit"},
    "three_d": {"gene", "source", "target", "cohort", "contrast", "omic", "limit"},
    "tcga_expression_survival": {"project", "lineage", "endpoint", "limit"},
    "tf_dependency": {"source", "target", "limit", "view"},
    "biomarker_target": set(),
}
LINEAGE_NETWORK_FAMILIES = {
    "effect_correlation",
    "expression_correlation",
    "expression_dependency",
}
MUTATION_EVENT_MATRIX = {
    "damaging": "Damaging",
    "damaging_mutation": "Damaging",
    "hotspot": "Hotspot",
    "hotspot_mutation": "Hotspot",
    "anyselected": "AnySelected",
    "custom_missense": "AnySelected",
    "custom_missense_mutation": "AnySelected",
}
ANCHOR_MODULE = "癌种内突变锚定基因选择"
OFFICIAL_MUTATION_DEPENDENCY = "depmap_official_gene_effect_v2"
QUERY_FIELD_ORDER = (
    "mode",
    "gene",
    "module",
    "source",
    "target",
    "limit",
    "event",
    "lineage",
    "pathway",
    "drug",
    "omic",
    "family",
    "ranking",
    "exclude_common_essential",
    "common_essential_source",
    "collection",
    "term",
    "contrast",
    "partner",
    "layer",
    "cohort",
    "reciprocal",
    "project",
    "endpoint",
    "catalog",
    "coverage",
    "view",
)
TCGA_SURVIVAL_ENDPOINTS = {"OS", "DSS", "DFI", "PFI"}
CANONICAL_LINEAGES = (
    "Adrenal Gland", "Ampulla of Vater", "Biliary Tract",
    "Bladder Urinary Tract", "Bone", "Bowel", "Breast", "Cervix",
    "CNS Brain", "Embryonal", "Esophagus Stomach", "Eye", "Fibroblast",
    "Hair", "Head and Neck", "Kidney", "Liver", "Lung", "Lymphoid",
    "Muscle", "Myeloid", "Normal", "Other", "Ovary Fallopian Tube",
    "Pancreas", "Peripheral Nervous System", "Pleura", "Prostate", "Skin",
    "Soft Tissue", "Testis", "Thyroid", "Uterus", "Vulva Vagina",
)
CHINESE_LINEAGE_ALIASES: dict[str, tuple[str, ...]] = {
    "Adrenal Gland": ("肾上腺", "肾上腺癌", "肾上腺肿瘤"),
    "Ampulla of Vater": ("Vater壶腹", "壶腹部", "壶腹癌", "壶腹部癌"),
    "Biliary Tract": ("胆道", "胆道癌", "胆管癌", "胆囊癌"),
    "Bladder Urinary Tract": ("膀胱", "膀胱癌", "尿路", "尿路癌", "尿路上皮癌"),
    "Bone": ("骨", "骨癌", "骨肿瘤", "骨肉瘤"),
    "Bowel": ("肠道", "肠癌", "大肠癌", "结肠癌", "直肠癌", "结直肠癌"),
    "Breast": ("乳腺", "乳腺癌", "乳癌"),
    "Cervix": ("宫颈", "宫颈癌", "子宫颈癌"),
    "CNS Brain": ("中枢神经系统", "脑", "脑癌", "脑肿瘤", "胶质瘤"),
    "Embryonal": ("胚胎性", "胚胎性肿瘤", "胚胎肿瘤"),
    "Esophagus Stomach": ("食管", "食管癌", "胃", "胃癌", "食管胃", "食管胃癌", "胃食管癌"),
    "Eye": ("眼", "眼部", "眼部肿瘤", "眼癌", "葡萄膜黑色素瘤"),
    "Fibroblast": ("成纤维细胞", "成纤维细胞系"),
    "Hair": ("毛发", "毛囊"),
    "Head and Neck": ("头颈", "头颈癌", "口腔癌", "咽癌", "喉癌"),
    "Kidney": ("肾", "肾癌", "肾脏癌", "肾细胞癌"),
    "Liver": ("肝", "肝癌", "肝脏癌", "肝脏肿瘤"),
    "Lung": ("肺", "肺癌", "肺部肿瘤"),
    "Lymphoid": ("淋巴", "淋巴系统", "淋巴系统肿瘤", "淋巴瘤", "淋巴细胞白血病"),
    "Muscle": ("肌肉", "肌肉肿瘤"),
    "Myeloid": ("髓系", "髓系肿瘤", "髓系白血病", "急性髓系白血病"),
    "Normal": ("正常", "正常组织", "正常细胞"),
    "Other": ("其他", "其他肿瘤"),
    "Ovary Fallopian Tube": ("卵巢", "卵巢癌", "输卵管", "输卵管癌", "卵巢输卵管"),
    "Pancreas": ("胰腺", "胰腺癌", "胰癌"),
    "Peripheral Nervous System": ("外周神经系统", "周围神经系统", "外周神经系统肿瘤", "神经母细胞瘤"),
    "Pleura": ("胸膜", "胸膜肿瘤", "胸膜间皮瘤", "间皮瘤"),
    "Prostate": ("前列腺", "前列腺癌"),
    "Skin": ("皮肤", "皮肤癌", "皮肤肿瘤", "黑色素瘤"),
    "Soft Tissue": ("软组织", "软组织肿瘤", "软组织肉瘤"),
    "Testis": ("睾丸", "睾丸癌", "睾丸肿瘤"),
    "Thyroid": ("甲状腺", "甲状腺癌", "甲状腺肿瘤"),
    "Uterus": ("子宫", "子宫癌", "子宫体癌", "子宫内膜癌"),
    "Vulva Vagina": ("外阴", "外阴癌", "阴道", "阴道癌", "外阴阴道"),
}


def _lineage_match_key(value: str) -> str:
    return re.sub(r"[^a-z0-9]+", "", value.strip().lower())


def _unicode_lineage_alias_key(value: str) -> str:
    return re.sub(r"[\s\-_/，、,（）()]+", "", value.strip().casefold())


CHINESE_LINEAGE_LOOKUP = {
    _unicode_lineage_alias_key(alias): canonical
    for canonical, aliases in CHINESE_LINEAGE_ALIASES.items()
    for alias in aliases
}

AMBIGUOUS_LINEAGE_TERMS: dict[str, tuple[str, ...]] = {
    "白血病": ("Myeloid", "Lymphoid"),
    "血液肿瘤": ("Myeloid", "Lymphoid"),
    "血液系统肿瘤": ("Myeloid", "Lymphoid"),
    "leukemia": ("Myeloid", "Lymphoid"),
    "hematologic cancer": ("Myeloid", "Lymphoid"),
    "肉瘤": ("Bone", "Soft Tissue", "Muscle"),
    "sarcoma": ("Bone", "Soft Tissue", "Muscle"),
    "妇科肿瘤": ("Cervix", "Ovary Fallopian Tube", "Uterus", "Vulva Vagina"),
    "gynecologic cancer": ("Cervix", "Ovary Fallopian Tube", "Uterus", "Vulva Vagina"),
    "神经系统肿瘤": ("CNS Brain", "Peripheral Nervous System"),
    "nervous system cancer": ("CNS Brain", "Peripheral Nervous System"),
    "消化系统肿瘤": (
        "Biliary Tract", "Bowel", "Esophagus Stomach", "Liver", "Pancreas",
    ),
    "消化道肿瘤": ("Bowel", "Esophagus Stomach"),
    "gastrointestinal cancer": (
        "Biliary Tract", "Bowel", "Esophagus Stomach", "Liver", "Pancreas",
    ),
    "泌尿生殖系统肿瘤": (
        "Bladder Urinary Tract", "Kidney", "Prostate", "Testis",
    ),
    "genitourinary cancer": (
        "Bladder Urinary Tract", "Kidney", "Prostate", "Testis",
    ),
}


def _canonical_lineage_label(value: str) -> str:
    requested = value.strip()
    unicode_alias = CHINESE_LINEAGE_LOOKUP.get(_unicode_lineage_alias_key(requested))
    if unicode_alias is not None:
        return unicode_alias
    canonical_by_key = {
        _lineage_match_key(candidate): candidate for candidate in CANONICAL_LINEAGES
    }
    direct = canonical_by_key.get(_lineage_match_key(requested))
    if direct is not None:
        return direct
    without_suffix = re.sub(
        r"\s+(?:cancer|carcinoma|tumors?|lineage)$", "", requested,
        flags=re.IGNORECASE,
    ).strip()
    direct = canonical_by_key.get(_lineage_match_key(without_suffix))
    if direct is not None:
        return direct
    aliases = {
        "brain": "CNS Brain", "cns": "CNS Brain",
        "centralnervoussystem": "CNS Brain", "colon": "Bowel",
        "colorectal": "Bowel", "rectal": "Bowel",
        "esophageal": "Esophagus Stomach", "gastric": "Esophagus Stomach",
        "stomach": "Esophagus Stomach", "headneck": "Head and Neck",
        "headandneck": "Head and Neck", "ovarian": "Ovary Fallopian Tube",
        "ovary": "Ovary Fallopian Tube", "fallopiantube": "Ovary Fallopian Tube",
        "pns": "Peripheral Nervous System", "bladder": "Bladder Urinary Tract",
        "urinarytract": "Bladder Urinary Tract", "vulvar": "Vulva Vagina",
        "vaginal": "Vulva Vagina", "vulva": "Vulva Vagina",
        "vagina": "Vulva Vagina",
    }
    return aliases.get(_lineage_match_key(without_suffix), requested)


def resolve_lineage_term(
    term: str,
    candidate_lineages: list[str] | None = None,
) -> dict[str, Any]:
    """Resolve exact labels and validate model-proposed ambiguous candidates.

    Exact maintained aliases are safe to select automatically. Broad terms and
    model-proposed candidates remain unselected until the user supplies one
    explicit lineage on a later turn.
    """

    requested = term.strip()
    if not requested:
        raise ValueError("term must be non-empty")

    ambiguous_by_key = {
        _unicode_lineage_alias_key(label): lineages
        for label, lineages in AMBIGUOUS_LINEAGE_TERMS.items()
    }
    known_candidates = ambiguous_by_key.get(_unicode_lineage_alias_key(requested))
    if known_candidates is None:
        canonical = _canonical_lineage_label(requested)
        if canonical in CANONICAL_LINEAGES:
            return {
                "status": "RESOLVED",
                "original_term": requested,
                "selected_lineage": canonical,
                "candidates": [canonical],
                "resolution_basis": "canonical_or_maintained_alias",
                "requires_user_confirmation": False,
                "proxy_note": "canonical lineage is a DepMap model-grouping proxy, not an exact clinical histology",
            }

    proposed: list[str] = []
    invalid: list[str] = []
    for value in candidate_lineages or []:
        canonical = _canonical_lineage_label(value)
        if canonical not in CANONICAL_LINEAGES:
            invalid.append(value)
        elif canonical not in proposed:
            proposed.append(canonical)

    if invalid:
        return {
            "status": "INVALID_CANDIDATES",
            "original_term": requested,
            "selected_lineage": None,
            "candidates": list(known_candidates or proposed),
            "invalid_candidates": invalid,
            "resolution_basis": "candidate_validation",
            "requires_user_confirmation": True,
            "reason": "one or more proposed labels are not canonical DepMap lineages or maintained aliases",
        }

    if known_candidates is not None:
        incompatible = [value for value in proposed if value not in known_candidates]
        if incompatible:
            return {
                "status": "INVALID_CANDIDATES",
                "original_term": requested,
                "selected_lineage": None,
                "candidates": list(known_candidates),
                "invalid_candidates": incompatible,
                "resolution_basis": "known_ambiguous_term",
                "requires_user_confirmation": True,
                "reason": "the proposed lineage is incompatible with the maintained ambiguity set",
            }
        candidates = proposed or list(known_candidates)
        return {
            "status": "AMBIGUOUS",
            "original_term": requested,
            "selected_lineage": None,
            "candidates": candidates,
            "resolution_basis": (
                "validated_model_proposal" if proposed else "known_ambiguous_term"
            ),
            "requires_user_confirmation": True,
            "reason": "choose one canonical DepMap proxy before querying evidence",
        }

    if proposed:
        return {
            "status": "PROPOSED",
            "original_term": requested,
            "selected_lineage": None,
            "candidates": proposed,
            "resolution_basis": "validated_model_proposal",
            "requires_user_confirmation": True,
            "reason": "candidate labels are valid, but their semantic match to the original term is not independently verified",
        }

    return {
        "status": "UNRESOLVED",
        "original_term": requested,
        "selected_lineage": None,
        "candidates": [],
        "resolution_basis": "none",
        "requires_user_confirmation": True,
        "reason": "no maintained alias or validated candidate lineage is available",
    }


@dataclass(frozen=True)
class Settings:
    knowledge_root: Path
    query_script: Path
    api_token: str
    release: str = "26Q1"
    timeout_seconds: float = 60.0
    max_concurrency: int = 2
    rscript: str = "Rscript"

    @classmethod
    def from_env(cls) -> "Settings":
        root = Path(os.environ["DEPMAP_KNOWLEDGE_ROOT"]).expanduser().resolve()
        script = Path(os.environ["DEPMAP_QUERY_SCRIPT"]).expanduser().resolve()
        token = os.environ["DEPMAP_API_TOKEN"]
        if len(token) < 32:
            raise RuntimeError("DEPMAP_API_TOKEN must contain at least 32 characters")
        return cls(
            knowledge_root=root,
            query_script=script,
            api_token=token,
            release=os.environ.get("DEPMAP_RELEASE", "26Q1"),
            timeout_seconds=float(os.environ.get("DEPMAP_QUERY_TIMEOUT_SECONDS", "60")),
            max_concurrency=int(os.environ.get("DEPMAP_MAX_CONCURRENCY", "2")),
            rscript=os.environ.get("RSCRIPT", "Rscript"),
        )


class QueryRequest(BaseModel):
    model_config = ConfigDict(extra="forbid")

    mode: Literal[
        "analysis_catalog",
        "mutation_anchor",
        "lineage_mutation_dependency",
        "catalog", "lineage_catalog", "lineage_dependency", "pan_cancer_dependency", "core", "pair", "top", "lineage", "pathway", "drug",
        "lineage_network", "lineage_cnv", "lineage_drug", "enrichment",
        "lineage_directions",
        "subtype", "coamplification",
        "true_love", "synthetic_lethal", "three_d",
        "tcga_expression_survival",
        "tf_dependency",
        "biomarker_target",
    ]
    gene: str | None = None
    module: str | None = None
    completion_state: Literal["COMPLETE", "UNVERIFIED"] | None = None
    anchor_tier: Literal["priority", "strict", "standard"] | None = None
    include_common_essential: bool | None = None
    exclude_common_essential: bool | None = None
    common_essential_source: Literal["depmap_26q1"] | None = None
    source: str | None = None
    target: str | None = None
    limit: int | None = Field(default=None, ge=1)
    event: str | None = None
    lineage: str | None = None
    pathway: str | None = None
    drug: str | None = None
    omic: str | None = None
    family: str | None = None
    ranking: Literal["selective", "mean_dependency"] | None = None
    collection: str | None = None
    term: str | None = None
    contrast: str | None = None
    partner: str | None = None
    layer: Literal["exhaustive_high_confidence", "lineage_adjusted"] | None = None
    cohort: str | None = None
    reciprocal: bool | None = None
    project: str | None = None
    endpoint: str | None = None
    catalog: Literal["stable_negative_rank1", "negative_r_lt_minus_0_3", "positive_reciprocal_top20"] | None = None
    coverage: Literal["all", "legacy", "quality"] | None = None
    view: Literal["universe", "ranking"] | None = None

    @model_validator(mode="after")
    def validate_mode_contract(self) -> "QueryRequest":
        required = MODE_REQUIRED_FIELDS[self.mode]
        allowed = required | MODE_OPTIONAL_FIELDS.get(self.mode, set())
        all_fields = {
            "gene", "module", "completion_state", "anchor_tier", "include_common_essential", "exclude_common_essential", "common_essential_source", "source", "target", "limit", "event", "lineage",
            "pathway", "drug", "omic", "family", "ranking", "collection", "term", "reciprocal",
            "project", "endpoint", "contrast", "partner", "layer", "cohort",
            "catalog", "coverage", "view",
        }
        supplied = {
            name
            for name in all_fields
            if getattr(self, name, None) is not None
        }
        missing = required - supplied
        unexpected = supplied - allowed
        if missing:
            raise ValueError(f"missing fields for {self.mode}: {', '.join(sorted(missing))}")
        if unexpected:
            raise ValueError(
                f"unexpected fields for {self.mode}: {', '.join(sorted(unexpected))}"
            )
        if self.module is not None and self.mode != "analysis_catalog" and self.module not in MATRIX_MODULES:
            raise ValueError("unsupported module")
        if (
            self.event is not None
            and self.mode not in {"synthetic_lethal", "lineage_mutation_dependency"}
            and self.event not in LINEAGE_EVENTS
        ):
            raise ValueError("unsupported lineage event")
        if self.mode == "synthetic_lethal" and self.event is not None and self.event not in SYNTHETIC_LETHAL_EVENTS:
            raise ValueError("unsupported synthetic-lethal event")
        if (
            self.mode == "lineage_mutation_dependency"
            and self.event is not None
            and self.event not in LINEAGE_EVENTS
            and self.event not in SYNTHETIC_LETHAL_EVENTS
            and self.event.casefold() not in MUTATION_EVENT_MATRIX
        ):
            raise ValueError("unsupported lineage mutation event")
        if self.omic is not None and self.mode != "three_d" and self.omic not in DRUG_OMICS:
            raise ValueError("unsupported drug omic")
        if self.mode == "three_d" and self.omic is not None and self.omic not in THREE_D_OMICS:
            raise ValueError("unsupported 3D omic")
        if self.family is not None and self.mode != "three_d" and self.family not in LINEAGE_NETWORK_FAMILIES:
            raise ValueError("unsupported lineage network family")
        if self.mode == "three_d" and self.family not in THREE_D_FAMILIES:
            raise ValueError("unsupported 3D family")
        if self.endpoint is not None and self.endpoint.upper() not in TCGA_SURVIVAL_ENDPOINTS:
            raise ValueError("unsupported TCGA survival endpoint")
        if self.mode == "lineage_drug" and self.drug is None and self.target is None:
            raise ValueError("lineage_drug requires drug, target, or both")
        if self.mode == "subtype" and self.gene is None and self.contrast is not None and self.limit is None:
            self.limit = 20
        if self.mode == "coamplification" and self.layer is None:
            self.layer = "lineage_adjusted"
        if self.mode == "true_love" and self.gene is None and self.partner is not None:
            raise ValueError("true_love partner requires gene")
        if self.mode == "true_love" and self.catalog in {None, "stable_negative_rank1"} and self.coverage is not None:
            raise ValueError("true_love coverage applies only to derived threshold or positive-reciprocal catalogs")
        if self.limit is not None:
            maximum = MODE_LIMIT_MAX.get(self.mode, 100)
            if self.limit > maximum:
                raise ValueError(f"limit must be between 1 and {maximum} for mode {self.mode}")
        if self.mode == "tf_dependency" and self.target is not None and self.source is None:
            raise ValueError("tf_dependency target requires source")
        if self.mode == "tf_dependency" and self.view == "universe" and self.source is not None:
            raise ValueError("tf_dependency universe view does not take a source")
        if self.mode == "lineage_mutation_dependency" and self.source is None and self.target is None:
            raise ValueError("lineage_mutation_dependency requires source, target, or both")
        for name in (supplied - {"limit", "reciprocal", "include_common_essential", "exclude_common_essential"}):
            value = getattr(self, name)
            if not isinstance(value, str) or not value.strip():
                raise ValueError(f"{name} must be a non-empty string")
            if len(value) > 256 or any(not char.isprintable() for char in value):
                raise ValueError(f"{name} must be at most 256 printable characters")
        return self

    def bounded_dict(self) -> dict[str, Any]:
        result = self.model_dump(exclude_none=True)
        if self.lineage is not None:
            result["lineage"] = _canonical_lineage_label(self.lineage)
        if self.project is not None:
            project = self.project.strip().upper()
            result["project"] = project if project.startswith("TCGA-") else f"TCGA-{project}"
        if self.endpoint is not None:
            result["endpoint"] = self.endpoint.strip().upper()
        return result


Runner = Callable[[Settings, dict[str, Any]], Awaitable[dict[str, Any]]]


def verify_installation(settings: Settings) -> dict[str, Any]:
    qa_path = settings.knowledge_root / "depmap-26q1-qa.json"
    required = (
        qa_path,
        settings.knowledge_root / "depmap-26q1-module-catalog.csv",
        settings.knowledge_root / "depmap-26q1-core",
        settings.knowledge_root / "depmap-26q1-full",
        settings.query_script,
    )
    missing = [str(path) for path in required if not path.exists()]
    if missing:
        raise RuntimeError(f"required DepMap paths are missing: {missing}")
    with qa_path.open(encoding="utf-8") as handle:
        qa = json.load(handle)
    if qa.get("qa_status") != "PASS":
        raise RuntimeError("knowledge QA status is not PASS")
    if qa.get("release") != settings.release:
        raise RuntimeError(
            f"knowledge release {qa.get('release')!r} does not match {settings.release!r}"
        )
    return qa


_COVERAGE_GAP_MARKERS = (
    "source not found or ineligible",
    "source block not found",
    "target not found",
    "drug or target not found",
)


def _coverage_gap_reason(stderr: str) -> str | None:
    for line in reversed(stderr.splitlines()):
        stripped = line.strip()
        lowered = stripped.lower()
        for marker in _COVERAGE_GAP_MARKERS:
            position = lowered.find(marker)
            if position >= 0:
                return stripped[position:]
    return None


def _coverage_gap(stderr: str) -> bool:
    return _coverage_gap_reason(stderr) is not None


def _r_query_command(settings: Settings, query: dict[str, Any]) -> list[str]:
    command = [
        settings.rscript,
        str(settings.query_script),
        "--kb-root",
        str(settings.knowledge_root),
    ]
    for key in QUERY_FIELD_ORDER:
        if key in query:
            command.extend((f"--{key.replace('_', '-')}", str(query[key])))
    return command


async def run_r_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    command = _r_query_command(settings, query)
    process = await asyncio.create_subprocess_exec(
        *command,
        cwd=str(settings.query_script.parent),
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
        env={
            **os.environ,
            "OMP_NUM_THREADS": os.environ.get("OMP_NUM_THREADS", "4"),
            "ARROW_NUM_THREADS": os.environ.get("ARROW_NUM_THREADS", "4"),
            "R_DATATABLE_NUM_PROCS_PERCENT": os.environ.get(
                "R_DATATABLE_NUM_PROCS_PERCENT", "10"
            ),
        },
    )
    try:
        stdout, stderr = await asyncio.wait_for(
            process.communicate(), timeout=settings.timeout_seconds
        )
    except TimeoutError:
        process.kill()
        await process.communicate()
        raise HTTPException(status_code=504, detail="bounded DepMap query timed out")
    stderr_text = stderr[-MAX_STDERR_BYTES:].decode("utf-8", errors="replace").strip()
    if process.returncode != 0:
        if _coverage_gap(stderr_text):
            return {
                "mode": query["mode"],
                "status": "not_testable",
                "reason": _coverage_gap_reason(stderr_text) or "not covered",
            }
        LOGGER.error("R query failed with exit code %s: %s", process.returncode, stderr_text)
        raise HTTPException(status_code=500, detail="DepMap query helper failed")
    if len(stdout) > MAX_RESPONSE_BYTES:
        raise HTTPException(status_code=413, detail="DepMap response exceeds 4 MiB")
    try:
        result = json.loads(stdout)
    except json.JSONDecodeError:
        LOGGER.error("R query returned invalid JSON: %s", stderr_text)
        raise HTTPException(status_code=500, detail="DepMap query returned invalid JSON")
    if len(json.dumps(result, ensure_ascii=False).encode("utf-8")) > MAX_RESPONSE_BYTES:
        raise HTTPException(status_code=413, detail="DepMap response exceeds 4 MiB")
    return result


def _run_core_query(settings: Settings, gene: str) -> dict[str, Any]:
    try:
        import pyarrow.parquet as parquet
    except ImportError:
        raise HTTPException(status_code=500, detail="PyArrow is required for core queries")
    symbol = gene.strip().upper()
    core = settings.knowledge_root / "depmap-26q1-core"
    summary_path = core / "gene_core_summary.parquet"
    summary = parquet.read_table(summary_path, filters=[("symbol", "=", symbol)]).to_pylist()
    lineages: list[dict[str, Any]] = []
    provenance = [str(summary_path)]
    for path in sorted((core / "lineage_blocks").glob("*.parquet")):
        rows = parquet.read_table(path, filters=[("symbol", "=", symbol)]).to_pylist()
        if rows:
            lineages.extend(rows)
            provenance.append(str(path))
    return {
        "mode": "core",
        "gene": symbol,
        "summary": summary,
        "lineages": lineages,
        "provenance": provenance,
    }


def _lineage_key(lineage: str) -> str:
    return re.sub(r"[^A-Za-z0-9]+", "_", lineage.strip()).strip("_")


def _load_manifest(path: Path) -> dict[str, Any] | None:
    manifest = path / "manifest.json"
    if not manifest.is_file():
        return None
    with manifest.open(encoding="utf-8-sig") as handle:
        return json.load(handle)


def _coerce_csv_value(value: str) -> Any:
    stripped = value.strip()
    if not stripped:
        return None
    if stripped == "TRUE":
        return True
    if stripped == "FALSE":
        return False
    if re.fullmatch(r"-?\d+", stripped):
        return int(stripped)
    if re.fullmatch(r"-?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?", stripped):
        return float(stripped)
    return value


def _read_csv_records(path: Path) -> list[dict[str, Any]]:
    return list(_iter_csv_records(path))


def _iter_csv_records(path: Path):
    opener = gzip.open if path.suffix == ".gz" else open
    with opener(path, "rt", encoding="utf-8-sig", newline="") as handle:
        for row in csv.DictReader(handle):
            yield {key: _coerce_csv_value(value) for key, value in row.items()}


def _filter_csv_records(
    path: Path,
    match,
    *,
    limit: int | None = None,
) -> list[dict[str, Any]]:
    """Scan a CSV and keep matching rows before applying any bound."""
    if not path.is_file():
        return []
    return filter_before_limit(_iter_csv_records(path), match, limit=limit)


def _mutation_event_matrix(event: str | None) -> str | None:
    if event is None:
        return None
    key = event.strip().casefold().replace("-", "_")
    if key in MUTATION_EVENT_MATRIX:
        return MUTATION_EVENT_MATRIX[key]
    if event.strip() in {"Damaging", "Hotspot", "AnySelected"}:
        return event.strip()
    return None


def _anchor_catalog_root(settings: Settings) -> Path:
    return (
        settings.knowledge_root / "analysis-modules" / ANCHOR_MODULE / "cancer_anchor_catalog_v2"
    )


def _lineage_anchor_root(settings: Settings, lineage: str) -> Path:
    return _anchor_catalog_root(settings) / "by_cancer" / _lineage_key(lineage)


def _int_field(row: dict[str, Any], *names: str) -> int | None:
    for name in names:
        value = row.get(name)
        if isinstance(value, bool) or value is None or value == "":
            continue
        try:
            return int(value)
        except (TypeError, ValueError):
            continue
    return None


def _bool_field(row: dict[str, Any], name: str) -> bool | None:
    value = row.get(name)
    if isinstance(value, bool):
        return value
    if value is None:
        return None
    text = str(value).strip().casefold()
    if text in {"true", "yes", "1"}:
        return True
    if text in {"false", "no", "0"}:
        return False
    return None


def _anchor_thresholds(manifest: dict[str, Any]) -> dict[str, dict[str, int]]:
    raw = manifest.get("thresholds") or {}
    standard = raw.get("standard") or {}
    strict = raw.get("strict") or {}
    return {
        "standard": {
            "min_mut": int(standard.get("min_mut") or 3),
            "min_wt": int(standard.get("min_wt") or 5),
        },
        "strict": {
            "min_mut": int(strict.get("min_mut") or 10),
            "min_wt": int(strict.get("min_wt") or 5),
        },
    }


def _evaluate_anchor_criteria(
    row: dict[str, Any],
    *,
    thresholds: dict[str, dict[str, int]],
    card: dict[str, Any] | None,
    requested_tier: str,
) -> dict[str, Any]:
    mut_n = _int_field(row, "mut_n")
    wt_n = _int_field(row, "wt_n")
    standard = thresholds["standard"]
    strict = thresholds["strict"]
    pass_standard = (
        mut_n is not None
        and wt_n is not None
        and mut_n >= standard["min_mut"]
        and wt_n >= standard["min_wt"]
    )
    pass_strict = (
        mut_n is not None
        and wt_n is not None
        and mut_n >= strict["min_mut"]
        and wt_n >= strict["min_wt"]
    )
    role_match = _bool_field(card or {}, "role_match")
    oncokb_role = (card or {}).get("oncokb_role")
    failures: list[str] = []
    needed = strict if requested_tier in {"priority", "strict"} else standard
    failures.extend(
        criteria_failures(
            observed={"mut": mut_n, "wt": wt_n},
            required={"mut": needed["min_mut"], "wt": needed["min_wt"]},
        )
    )
    if requested_tier == "priority" and "TOO_FEW_MUT" not in failures and "TOO_FEW_WT" not in failures:
        if role_match is False:
            failures.append("ROLE_MISMATCH")
        elif card is None:
            failures.append("ANNOTATION_EXCLUSION")
    rejection = failures[0] if failures else None
    size_ok = rejection not in {"TOO_FEW_MUT", "TOO_FEW_WT", "COUNTS_UNAVAILABLE"}
    retained = rejection not in {"ROLE_MISMATCH", "ANNOTATION_EXCLUSION"}
    status = classify_exact_entity(
        observed=True,
        eligible=size_ok,
        retained=retained if size_ok else None,
    )
    return {
        "gene": str(row.get("gene") or "").upper(),
        "lineage": row.get("lineage"),
        "matrix": row.get("matrix"),
        "event_definition": row.get("matrix"),
        "mut_n": mut_n,
        "wt_n": wt_n,
        "cohort_n": _int_field(row, "cohort_n"),
        "mut_rate": row.get("mut_rate"),
        "is_common_essential": _bool_field(row, "is_common_essential"),
        "oncokb_role": oncokb_role,
        "role_match": role_match,
        "thresholds": thresholds,
        "pass_standard": pass_standard,
        "pass_strict": pass_strict,
        "requested_tier": requested_tier,
        "criteria": {
            "standard_mut": {
                "threshold": standard["min_mut"],
                "observed": mut_n,
                "pass": mut_n is not None and mut_n >= standard["min_mut"],
            },
            "standard_wt": {
                "threshold": standard["min_wt"],
                "observed": wt_n,
                "pass": wt_n is not None and wt_n >= standard["min_wt"],
            },
            "strict_mut": {
                "threshold": strict["min_mut"],
                "observed": mut_n,
                "pass": mut_n is not None and mut_n >= strict["min_mut"],
            },
            "strict_wt": {
                "threshold": strict["min_wt"],
                "observed": wt_n,
                "pass": wt_n is not None and wt_n >= strict["min_wt"],
            },
            "role_match": role_match,
        },
        "criterion_failures": failures,
        "rejection_reason": rejection,
        "eligibility_status": status,
    }


def _indexed_true_love_rows(
    settings: Settings, *, catalog: str, coverage: str,
    gene: str | None, partner: str | None, limit: int,
) -> list[dict[str, Any]] | None:
    """Return bounded indexed rows, or None when the optional index is unavailable."""
    path = settings.knowledge_root / "depmap-26q1-query-index.sqlite"
    if not path.is_file():
        return None
    symbol = gene.strip().upper() if gene else None
    mate = partner.strip().upper() if partner else None
    clauses = ["catalog = ?", "coverage = ?"]
    params: list[Any] = [catalog, coverage]
    if symbol:
        clauses.append("(gene_a = ? OR gene_b = ?)")
        params.extend((symbol, symbol))
    if mate:
        clauses.append("(gene_a = ? OR gene_b = ?)")
        params.extend((mate, mate))
    params.append(limit)
    uri = f"file:{path.as_posix()}?mode=ro&immutable=1"
    db: sqlite3.Connection | None = None
    try:
        db = sqlite3.connect(uri, uri=True)
        rows = db.execute(
                f"SELECT row_json FROM true_love WHERE {' AND '.join(clauses)} "
                "ORDER BY sort_1, sort_2 LIMIT ?", params,
        ).fetchall()
    except (sqlite3.Error, OSError):
        LOGGER.exception("DepMap query index unavailable; falling back to source files")
        return None
    finally:
        if db is not None:
            db.close()
    return [{key: _coerce_csv_value(value) for key, value in json.loads(row[0]).items()} for row in rows]


def _run_biomarker_target_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    target = query["target"].strip().upper()
    index = settings.knowledge_root / "depmap-26q1-query-index.sqlite"
    row: dict[str, Any] | None = None
    used_index = False
    if index.is_file():
        db: sqlite3.Connection | None = None
        try:
            db = sqlite3.connect(f"file:{index.as_posix()}?mode=ro&immutable=1", uri=True)
            hit = db.execute(
                "SELECT row_json FROM biomarker_target WHERE target_gene = ?", (target,)
            ).fetchone()
            if hit:
                row = {key: _coerce_csv_value(value) for key, value in json.loads(hit[0]).items()}
            used_index = True
        except (sqlite3.Error, OSError):
            LOGGER.exception("Biomarker target index unavailable; falling back to CSV")
        finally:
            if db is not None:
                db.close()
    module = settings.knowledge_root / "analysis-modules" / "表达基因-CRISPR基因依赖相关性分析"
    catalog = module / "results" / "predictive_biomarker" / "target_eligibility_26Q1" / "target_eligibility_catalog.csv"
    if row is None and not used_index and catalog.is_file():
        row = next((item for item in _read_csv_records(catalog) if item.get("target_gene") == target), None)
    cache = module / "results" / "predictive_biomarker" / f"{target}_26Q1"
    validation = cache / "validation.json"
    cached = validation.is_file()
    return _evidence_response(
        "FOUND" if row else "NOT_COMPUTED", mode="biomarker_target",
        reason=("target eligibility and cached-model state found" if row else "target is absent from the Gene Effect modeling catalog"),
        target=target, eligibility=row, cached_model=cached,
        cached_model_status=(json.loads(validation.read_text(encoding="utf-8-sig")).get("status") if cached else None),
        execution_policy="nested LASSO/random forest is run on demand per target and cached; eligibility is prioritization, not exclusion",
        entrypoint="analysis-modules/表达基因-CRISPR基因依赖相关性分析/scripts/build_predictive_biomarker_model.R",
        provenance=[str(catalog), *([str(index)] if used_index else []), *([str(validation)] if cached else [])],
    )


def _run_analysis_catalog_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    index = settings.knowledge_root / "depmap-26q1-query-index.sqlite"
    if not index.is_file():
        return _evidence_response(
            "MODULE_UNAVAILABLE", mode="analysis_catalog",
            reason="the unified directory index is not installed",
        )
    clauses: list[str] = []
    params: list[Any] = []
    module = query.get("module")
    state = query.get("completion_state") or "COMPLETE"
    invalid_capability_record_count = 0
    if module:
        module_patterns: list[str] = []
        try:
            with closing(sqlite3.connect(
                f"file:{index.as_posix()}?mode=ro&immutable=1", uri=True
            )) as catalog_db:
                row = catalog_db.execute(
                    "SELECT payload_json FROM capability_catalog "
                    "WHERE query_mode = ? OR intent = ? LIMIT 1",
                    (module, module),
                ).fetchone()
                if row:
                    details = json.loads(str(row[0]))
                    if isinstance(details, dict):
                        patterns = details.get("inventory_patterns") or []
                        if isinstance(patterns, list):
                            module_patterns = [
                                str(part).strip().replace("*", "%")
                                for part in patterns if str(part).strip()
                            ]
                    else:
                        invalid_capability_record_count += 1
        except (sqlite3.Error, json.JSONDecodeError, TypeError, AttributeError):
            module_patterns = []
            invalid_capability_record_count += 1
        module_clauses = ["module = ?", "analysis_unit = ?"]
        params.extend([module, module])
        for pattern in module_patterns:
            module_clauses.extend(["module LIKE ?", "analysis_unit LIKE ?"])
            params.extend([pattern, pattern])
        clauses.append("(" + " OR ".join(module_clauses) + ")")
    if state:
        clauses.append("completion_state = ?")
        params.append(state)
    limit = min(int(query.get("limit") or 100), 500)
    sql = (
        "SELECT analysis_id,module,analysis_unit,completion_state,completion_basis,"
        "release,family,dataset,method,manifest_path FROM analysis_catalog"
    )
    if clauses:
        sql += " WHERE " + " AND ".join(clauses)
    sql += " ORDER BY module,analysis_unit LIMIT ?"
    params.append(limit)
    try:
        with closing(sqlite3.connect(
            f"file:{index.as_posix()}?mode=ro&immutable=1", uri=True
        )) as db:
            db.row_factory = sqlite3.Row
            rows = [dict(row) for row in db.execute(sql, params)]
            totals = {
                row[0]: row[1]
                for row in db.execute(
                    "SELECT completion_state,COUNT(*) FROM analysis_catalog GROUP BY completion_state"
                )
            }
    except sqlite3.Error as exc:
        return _evidence_response(
            "MODULE_UNAVAILABLE", mode="analysis_catalog",
            reason=f"the unified directory index could not be read: {exc}",
        )
    return _evidence_response(
        "FOUND" if rows else "NOT_RETAINED", mode="analysis_catalog",
        reason="completed analysis directory entries from the unified relative-path catalog",
        rows=rows, returned_count=len(rows), state_totals=totals,
        catalog_status=("PARTIAL" if invalid_capability_record_count else "FOUND"),
        invalid_record_count=invalid_capability_record_count,
        path_policy="knowledge-root-relative paths only",
        provenance=[index.name],
    )


def _lookup_anchor_card(root: Path, gene: str, matrix: str | None) -> dict[str, Any] | None:
    path = root / "data" / "anchor_gene_cards_standard.csv"
    rows = _filter_csv_records(
        path,
        lambda row: str(row.get("gene") or "").upper() == gene
        and (matrix is None or str(row.get("matrix") or "") == matrix),
        limit=1,
    )
    return rows[0] if rows else None


def _exact_anchor_eligibility(
    settings: Settings,
    *,
    lineage: str,
    gene: str,
    event: str | None,
    tier: str,
    root: Path,
    manifest: dict[str, Any],
    manifest_path: Path,
) -> dict[str, Any]:
    menu_path = root / "data" / "gene_by_lineage_mutation_menu.csv"
    if not menu_path.is_file():
        return _evidence_response(
            "COVERAGE_GAP",
            mode="mutation_anchor",
            lineage=lineage,
            gene=gene,
            event=event,
            reason="the lineage catalog is installed but the mutation-count menu is missing",
            rejection_reason="COVERAGE_GAP",
            manifest=manifest,
            provenance=[str(manifest_path)],
        )
    matrix = _mutation_event_matrix(event)
    gene_rows = _filter_csv_records(
        menu_path,
        lambda row: str(row.get("gene") or "").upper() == gene,
    )
    if not gene_rows:
        return _evidence_response(
            "NOT_OBSERVED",
            mode="mutation_anchor",
            lineage=lineage,
            gene=gene,
            event=event,
            reason="the gene is absent from the lineage mutation-count menu; absence from a retained candidate list is not a numeric proof",
            rejection_reason="NOT_OBSERVED",
            wording="not retained until an explicit eligibility record is available",
            manifest=manifest,
            provenance=[str(manifest_path), str(menu_path)],
        )
    if event and matrix is None:
        return _evidence_response(
            "NOT_COMPUTED",
            mode="mutation_anchor",
            lineage=lineage,
            gene=gene,
            event=event,
            reason="the requested event class is not part of the lineage mutation-anchor menu",
            rejection_reason="UNSUPPORTED_EVENT",
            manifest=manifest,
            provenance=[str(manifest_path), str(menu_path)],
        )
    matched = [
        row for row in gene_rows
        if matrix is None or str(row.get("matrix") or "") == matrix
    ]
    if event and not matched:
        return _evidence_response(
            "NOT_OBSERVED",
            mode="mutation_anchor",
            lineage=lineage,
            gene=gene,
            event=event,
            observed_events=[str(row.get("matrix")) for row in gene_rows],
            reason="the requested event class was not observed for this gene in the lineage menu",
            rejection_reason="ABSENT_EVENT",
            wording="not retained until an explicit eligibility record is available",
            manifest=manifest,
            provenance=[str(manifest_path), str(menu_path)],
        )
    thresholds = _anchor_thresholds(manifest)
    records = []
    for row in matched:
        card = _lookup_anchor_card(root, gene, str(row.get("matrix") or "") or None)
        record = _evaluate_anchor_criteria(
            {**row, "cohort_n": manifest.get("cohort_n")},
            thresholds=thresholds,
            card=card,
            requested_tier=tier,
        )
        records.append(record)
    primary = records[0]
    status = primary["eligibility_status"]
    if any(item["eligibility_status"] == "FOUND" for item in records) and matrix is None:
        status = "FOUND"
        primary = next(item for item in records if item["eligibility_status"] == "FOUND")
    reasons = {
        "FOUND": "exact mutation-anchor eligibility record from the lineage mutation-count menu",
        "INELIGIBLE": "the requested gene fails the recorded Mut/WT sample-size thresholds",
        "NOT_RETAINED": "counts are available, but the requested tier excludes the gene by role or annotation policy",
    }
    return _evidence_response(
        status,
        mode="mutation_anchor",
        lineage=lineage,
        gene=gene,
        event=event,
        anchor_tier=tier,
        reason=reasons[status],
        rejection_reason=primary.get("rejection_reason"),
        eligibility=primary,
        rows=records,
        returned_count=len(records),
        cohort_n=manifest.get("cohort_n"),
        thresholds=manifest.get("thresholds") or thresholds,
        event_definitions=manifest.get("event_definitions"),
        wording="use the returned rejection_reason; do not infer a numeric bound from a missing retained-candidate row",
        manifest=manifest,
        provenance=[str(manifest_path), str(menu_path)],
    )


def _run_mutation_anchor_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    lineage = _canonical_lineage_label(query["lineage"])
    root = _lineage_anchor_root(settings, lineage)
    manifest_path = root / "manifest.json"
    if not manifest_path.is_file():
        return _evidence_response(
            "MODULE_UNAVAILABLE", mode="mutation_anchor", lineage=lineage,
            reason="the requested lineage has no completed mutation-anchor catalog",
        )
    manifest = json.loads(manifest_path.read_text(encoding="utf-8-sig"))
    if str(manifest.get("status", "")).lower() != "complete":
        return _evidence_response(
            "NOT_COMPUTED", mode="mutation_anchor", lineage=lineage,
            reason="the mutation-anchor catalog is not marked complete", manifest=manifest,
        )
    tier = query.get("anchor_tier") or "priority"
    event = query.get("event")
    gene = str(query["gene"]).strip().upper() if query.get("gene") else None
    if gene:
        return _exact_anchor_eligibility(
            settings,
            lineage=lineage,
            gene=gene,
            event=event,
            tier=tier,
            root=root,
            manifest=manifest,
            manifest_path=manifest_path,
        )
    relative = {
        "priority": "results/priority_role_matched_candidates.csv",
        "strict": "results/strict_functional_candidates.csv",
        "standard": "results/functional_candidates.csv",
    }[tier]
    path = root / relative
    matrix = _mutation_event_matrix(event)
    include_ce = bool(query.get("include_common_essential", False))
    limit = min(int(query.get("limit") or 20), 100)
    rows = _filter_csv_records(
        path,
        lambda row: (
            (matrix is None or str(row.get("matrix") or "") == matrix)
            and (include_ce or str(row.get("is_common_essential") or "").upper() != "TRUE")
        ),
        limit=limit,
    )
    return _evidence_response(
        "FOUND" if rows else "NOT_RETAINED", mode="mutation_anchor", lineage=lineage,
        reason="eligible mutation anchors with analyzable Mut/WT support; candidate status is not a dependency association",
        anchor_tier=tier, event=event, rows=rows, returned_count=len(rows),
        cohort_n=manifest.get("cohort_n"), thresholds=manifest.get("thresholds"),
        event_definitions=manifest.get("event_definitions"), manifest=manifest,
        provenance=[str(manifest_path), str(path)],
    )


def _read_parquet_records(
    path: Path,
    *,
    filters: list[tuple[str, str, Any]] | None = None,
) -> list[dict[str, Any]]:
    import pyarrow.parquet as pq

    table = pq.read_table(path, filters=filters or None)
    return table.to_pylist()


def _official_dependency_root(settings: Settings, lineage: str) -> Path:
    return (
        _lineage_anchor_root(settings, lineage)
        / "dependency_analysis"
        / OFFICIAL_MUTATION_DEPENDENCY
    )


def _run_lineage_mutation_dependency_query(
    settings: Settings, query: dict[str, Any]
) -> dict[str, Any]:
    lineage = _canonical_lineage_label(query["lineage"])
    source = str(query["source"]).strip().upper() if query.get("source") else None
    target = str(query["target"]).strip().upper() if query.get("target") else None
    event = query.get("event")
    matrix = _mutation_event_matrix(event)
    if event and matrix is None:
        return _evidence_response(
            "NOT_COMPUTED",
            mode="lineage_mutation_dependency",
            lineage=lineage,
            source=source,
            target=target,
            event=event,
            provider="lineage_official_gene_effect_v2",
            reason="the requested event class is not part of the lineage official mutation-dependency analysis",
        )
    if event and matrix == "AnySelected":
        return _evidence_response(
            "NOT_COMPUTED",
            mode="lineage_mutation_dependency",
            lineage=lineage,
            source=source,
            target=target,
            event=event,
            provider="lineage_official_gene_effect_v2",
            reason="AnySelected/custom-missense is a landscape event, not a completed official Gene Effect contrast",
        )
    anchor_root = _lineage_anchor_root(settings, lineage)
    anchor_manifest_path = anchor_root / "manifest.json"
    if not anchor_manifest_path.is_file():
        return _evidence_response(
            "MODULE_UNAVAILABLE",
            mode="lineage_mutation_dependency",
            lineage=lineage,
            source=source,
            target=target,
            event=event,
            provider="lineage_official_gene_effect_v2",
            reason="the requested lineage has no completed mutation-anchor catalog",
        )
    official_root = _official_dependency_root(settings, lineage)
    manifest, unavailable = _complete_module(official_root, mode="lineage_mutation_dependency")
    if unavailable is not None:
        unavailable["provider"] = "lineage_official_gene_effect_v2"
        unavailable["lineage"] = lineage
        unavailable["source"] = source
        unavailable["target"] = target
        unavailable["event"] = event
        return unavailable
    provenance = [str(anchor_manifest_path), str(official_root / "manifest.json")]
    anchor_manifest = json.loads(anchor_manifest_path.read_text(encoding="utf-8-sig"))
    eligibility = None
    if source:
        eligibility_result = _exact_anchor_eligibility(
            settings,
            lineage=lineage,
            gene=source,
            event=event,
            tier="standard",
            root=anchor_root,
            manifest=anchor_manifest,
            manifest_path=anchor_manifest_path,
        )
        eligibility = eligibility_result.get("eligibility") or (
            (eligibility_result.get("rows") or [None])[0]
        )
        if eligibility_result["status"] in {
            "INELIGIBLE",
            "NOT_OBSERVED",
            "COVERAGE_GAP",
            "MODULE_UNAVAILABLE",
        }:
            eligibility_result["mode"] = "lineage_mutation_dependency"
            eligibility_result["provider"] = "lineage_official_gene_effect_v2"
            eligibility_result["source"] = source
            eligibility_result["target"] = target
            eligibility_result["claim_strength"] = "hypothesis_generating"
            return eligibility_result
    pairs_path = official_root / "all_pairs.parquet"
    if not pairs_path.is_file():
        return _evidence_response(
            "COVERAGE_GAP",
            mode="lineage_mutation_dependency",
            lineage=lineage,
            source=source,
            target=target,
            event=event,
            provider="lineage_official_gene_effect_v2",
            eligibility=eligibility,
            reason="the official lineage mutation-dependency analysis is complete but all_pairs.parquet is missing",
            rejection_reason="COVERAGE_GAP",
            manifest=manifest,
            provenance=provenance,
        )
    filters: list[tuple[str, str, Any]] = [("lineage", "=", lineage)]
    if source:
        filters.append(("anchor_gene", "=", source))
    if target:
        filters.append(("dependency_gene", "=", target))
    if matrix in {"Damaging", "Hotspot"}:
        filters.append(("event_type", "=", matrix))
    rows = _read_parquet_records(pairs_path, filters=filters)
    provenance.append(str(pairs_path))
    if source and not rows:
        summary_path = official_root / "anchor_summary.csv"
        tested = _filter_csv_records(
            summary_path,
            lambda row: str(row.get("anchor_gene") or "").upper() == source
            and (matrix not in {"Damaging", "Hotspot"} or str(row.get("event_type") or "") == matrix),
            limit=1,
        )
        if summary_path.is_file():
            provenance.append(str(summary_path))
        if not tested:
            return _evidence_response(
                "NOT_COMPUTED",
                mode="lineage_mutation_dependency",
                lineage=lineage,
                source=source,
                target=target,
                event=event,
                provider="lineage_official_gene_effect_v2",
                eligibility=eligibility,
                reason="the mutation anchor is catalogued but was not tested in the completed official Gene Effect analysis",
                manifest=manifest,
                provenance=provenance,
            )
        return _evidence_response(
            "NOT_RETAINED",
            mode="lineage_mutation_dependency",
            lineage=lineage,
            source=source,
            target=target,
            event=event,
            provider="lineage_official_gene_effect_v2",
            eligibility=eligibility,
            reason="the lineage official analysis tested this anchor, but the requested target is absent from all tested pairs",
            group_definition="mutation-positive versus mutation-matrix-negative within the lineage",
            claim_strength="hypothesis_generating",
            manifest=manifest,
            provenance=provenance,
        )
    rows.sort(
        key=lambda row: (
            float(row.get("fdr_by_anchor") if row.get("fdr_by_anchor") is not None else 1),
            float(row.get("delta_gene_effect") if row.get("delta_gene_effect") is not None else 0),
            str(row.get("dependency_gene") or row.get("anchor_gene") or ""),
        )
    )
    limit = min(int(query.get("limit") or 20), 100)
    bounded = rows[:limit]
    return _evidence_response(
        "FOUND" if bounded else "NOT_RETAINED",
        mode="lineage_mutation_dependency",
        lineage=lineage,
        source=source,
        target=target,
        event=event or matrix,
        provider="lineage_official_gene_effect_v2",
        provider_scope="lineage",
        pan_cancer_provider="observational_synthetic_lethal",
        eligibility=eligibility,
        reason=(
            "lineage-scoped mutation-positive versus matrix-negative Gene Effect evidence"
            if bounded else
            "no matching lineage official mutation-dependency row was retained"
        ),
        rows=bounded,
        returned_count=len(bounded),
        matched_row_count=len(rows),
        group_definition="mutation-positive versus mutation-matrix-negative within the lineage",
        claim_strength="hypothesis_generating",
        interpretation="negative delta_gene_effect means stronger dependency in mutant models; this is observational and not causal synthetic lethality",
        manifest=manifest,
        provenance=provenance,
    )


def _complete_module(
    root: Path, *, mode: str
) -> tuple[dict[str, Any] | None, dict[str, Any] | None]:
    if not root.is_dir():
        return None, _evidence_response(
            "MODULE_UNAVAILABLE",
            mode=mode,
            reason="the requested precomputed module is not installed",
            provenance=[str(root.parent)],
        )
    manifest = _load_manifest(root)
    if manifest is None or manifest.get("status") != "complete":
        return manifest, _evidence_response(
            "NOT_COMPUTED",
            mode=mode,
            reason="the module has no complete terminal manifest",
            manifest=manifest,
            provenance=[str(root)],
        )
    return manifest, None


def _run_subtype_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    root = settings.knowledge_root / "depmap-26q1-full" / "subtype_dependency"
    manifest, unavailable = _complete_module(root, mode="subtype")
    if unavailable is not None:
        return unavailable
    catalog_path = root / "contrast_catalog.csv"
    if not catalog_path.is_file():
        return _evidence_response(
            "NOT_COMPUTED", mode="subtype",
            reason="the completed subtype module has no contrast catalog",
            manifest=manifest, provenance=[str(root / "manifest.json")],
        )
    lineage = query.get("lineage")
    contrast = query.get("contrast")
    gene = query.get("gene")
    limit = int(query.get("limit", 20))
    catalog = [row for row in _read_csv_records(catalog_path) if row.get("eligible") is True]
    selected = [
        row for row in catalog
        if (lineage is None or row.get("lineage") == lineage)
        and (contrast is None or str(row.get("contrast_id", "")).casefold() == contrast.casefold())
    ]
    provenance = [str(root / "manifest.json"), str(catalog_path)]
    if not selected:
        return _evidence_response(
            "NOT_COMPUTED", mode="subtype",
            reason="no eligible precomputed subtype contrast matches the request",
            gene=gene.upper() if gene else None, lineage=lineage, contrast=contrast,
            manifest=manifest, provenance=provenance,
        )
    if gene is None and contrast is None:
        return _evidence_response(
            "FOUND", mode="subtype",
            reason="eligible precomputed subtype contrasts found",
            lineage=lineage, rows=selected[:limit],
            summary={"matched_contrast_count": len(selected), "returned_count": min(limit, len(selected))},
            manifest=manifest, provenance=provenance,
        )

    rows: list[dict[str, Any]] = []
    symbol = gene.strip().upper() if gene else None
    for item in selected:
        unit = root / str(item["contrast_id"])
        path = unit / ("all_genes.csv.gz" if symbol else "selective_hits.csv.gz")
        unit_manifest = _load_manifest(unit)
        if not path.is_file() or unit_manifest is None or unit_manifest.get("status") != "complete":
            continue
        candidates = _read_csv_records(path)
        if symbol:
            candidates = [row for row in candidates if row.get("gene") == symbol]
        rows.extend(candidates)
        provenance.extend((str(unit / "manifest.json"), str(path)))
        if len(rows) >= limit:
            break
    rows = rows[:limit]
    if rows:
        return _evidence_response(
            "FOUND", mode="subtype",
            reason=("precomputed per-gene subtype rows found" if symbol else "retained selective subtype hits found"),
            gene=symbol, lineage=lineage, contrast=contrast, rows=rows,
            summary={"matched_contrast_count": len(selected), "returned_count": len(rows)},
            manifest=manifest, provenance=provenance,
        )
    return _evidence_response(
        "NOT_RETAINED" if symbol is None else "NOT_COMPUTED",
        mode="subtype",
        reason=(
            "the contrast is complete but has no retained selective dependency hits"
            if symbol is None else "the gene is absent from the completed subtype target universe"
        ),
        gene=symbol, lineage=lineage, contrast=contrast,
        manifest=manifest, provenance=provenance,
    )


def _run_coamplification_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    root = settings.knowledge_root / "depmap-26q1-full" / "coamplification_dependency"
    layer = query.get("layer", "lineage_adjusted")
    layer_root = root / layer
    manifest, unavailable = _complete_module(layer_root, mode="coamplification")
    if unavailable is not None:
        return unavailable
    catalog_path = root / "exhaustive_high_confidence" / "screen_pair_catalog.csv.gz"
    if not catalog_path.is_file():
        return _evidence_response(
            "NOT_COMPUTED", mode="coamplification",
            reason="the high-confidence directional pair catalog is unavailable",
            manifest=manifest, provenance=[str(layer_root / "manifest.json")],
        )
    source = query["source"].strip().upper()
    partner = query.get("partner")
    partner = partner.strip().upper() if partner else None
    target = query.get("target")
    target = target.strip().upper() if target else None
    limit = int(query.get("limit", 20))
    pairs = [
        row for row in _read_csv_records(catalog_path)
        if row.get("source_gene") == source
        and (partner is None or row.get("partner_gene") == partner)
    ]
    pairs.sort(key=lambda row: (-int(row.get("coamplified_n") or 0), -float(row.get("jaccard") or 0), str(row.get("partner_gene"))))
    provenance = [str(layer_root / "manifest.json"), str(catalog_path)]
    if not pairs:
        return _evidence_response(
            "NOT_COMPUTED", mode="coamplification",
            reason="the directed pair did not enter the constrained high-confidence screen",
            source=source, partner=partner, target=target, layer=layer,
            manifest=manifest, provenance=provenance,
        )
    selected_ids = {str(row["screen_pair_id"]) for row in pairs}
    audit_rows: list[dict[str, Any]] = []
    if layer == "lineage_adjusted":
        audit_path = layer_root / "pair_lineage_audit.csv.gz"
        if audit_path.is_file():
            audit_rows = [
                row for row in _read_csv_records(audit_path)
                if str(row.get("screen_pair_id")) in selected_ids
            ]
            provenance.append(str(audit_path))
        estimable_ids = {
            str(row["screen_pair_id"]) for row in audit_rows if row.get("estimable") is True
        }
        if partner is not None and not (selected_ids & estimable_ids):
            return _evidence_response(
                "INELIGIBLE", mode="coamplification",
                reason="the screened pair does not meet the lineage-adjusted informative-sample contract",
                source=source, partner=partner, target=target, layer=layer,
                pairs=pairs[:limit], audit=audit_rows[:limit],
                manifest=manifest, provenance=provenance,
            )
    hits_path = layer_root / "significant_hits.csv.gz"
    hits: list[dict[str, Any]] = []
    if hits_path.is_file():
        hits = [
            row for row in _read_csv_records(hits_path)
            if str(row.get("screen_pair_id")) in selected_ids
            and (target is None or row.get("target_gene") == target)
        ]
        fdr_field = "fdr_within_pair" if layer == "lineage_adjusted" else "fdr_coamplified_more_dependent"
        hits.sort(key=lambda row: (float(row.get(fdr_field) or 1), int(row.get("rank_within_pair") or 10**9)))
        provenance.append(str(hits_path))
    if partner is None and target is None:
        return _evidence_response(
            "FOUND", mode="coamplification",
            reason="bounded high-confidence coamplification partners found",
            source=source, partner=None, target=None, layer=layer,
            pairs=pairs[:limit], hits=hits[:limit], audit=audit_rows[:limit],
            summary={"matched_pair_count": len(pairs), "retained_hit_count": len(hits)},
            manifest=manifest, provenance=provenance,
        )
    status_name = "FOUND" if hits else "NOT_RETAINED"
    return _evidence_response(
        status_name, mode="coamplification",
        reason=(
            "retained precomputed coamplification dependency hits found"
            if hits else "the screened pair was computed but no matching dependency hit passed the retained-result contract"
        ),
        source=source, partner=partner, target=target, layer=layer,
        pairs=pairs[:limit], hits=hits[:limit], audit=audit_rows[:limit],
        summary={"matched_pair_count": len(pairs), "retained_hit_count": len(hits)},
        manifest=manifest, provenance=provenance,
    )


def _filter_pair_rows(
    rows: list[dict[str, Any]], gene: str | None, partner: str | None
) -> list[dict[str, Any]]:
    symbol = gene.strip().upper() if gene else None
    mate = partner.strip().upper() if partner else None
    filtered = []
    for row in rows:
        a = str(row.get("gene_a") or row.get("source_gene") or "").upper()
        b = str(row.get("gene_b") or row.get("target_gene") or "").upper()
        if symbol and symbol not in {a, b}:
            continue
        if mate and mate not in {a, b}:
            continue
        filtered.append(row)
    return filtered


def _run_true_love_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    root = settings.knowledge_root / "depmap-26q1-full" / "true_love_gene"
    manifest, unavailable = _complete_module(root, mode="true_love")
    if unavailable is not None:
        return unavailable
    catalog = query.get("catalog") or "stable_negative_rank1"
    coverage = query.get("coverage") or "all"
    stable_root = root / "high_confidence_stability"
    stable_manifest = _load_manifest(stable_root)
    stable_path = stable_root / "final_high_confidence_true_love_genes.csv.gz"
    strict_path = root / "strict_mutual_rank1_pairs.csv.gz"
    selected_manifest = manifest
    if catalog == "stable_negative_rank1":
        path = stable_path if stable_manifest and stable_manifest.get("status") == "complete" and stable_path.is_file() else strict_path
        selected_manifest = stable_manifest if path == stable_path else manifest
    else:
        derived_root = root / "tm00_derived_catalogs_26Q1"
        derived_manifest = _load_manifest(derived_root)
        if not derived_manifest or derived_manifest.get("status") != "complete":
            return _evidence_response("NOT_COMPUTED", mode="true_love", reason="the requested TM00-derived TLG catalog is not complete", catalog=catalog, coverage=coverage, manifest=derived_manifest, provenance=[str(derived_root / "manifest.json")])
        names = {
            ("negative_r_lt_minus_0_3", "legacy"): "negative_codependency_r_lt_minus_0.3_legacy.csv.gz",
            ("negative_r_lt_minus_0_3", "quality"): "negative_codependency_r_lt_minus_0.3_n500.csv.gz",
            ("positive_reciprocal_top20", "legacy"): "positive_reciprocal_top20_legacy.csv.gz",
            ("positive_reciprocal_top20", "quality"): "positive_reciprocal_top20_n500.csv.gz",
        }
        storage_coverage = "legacy" if coverage == "all" else coverage
        path = derived_root / names[(catalog, storage_coverage)]
        selected_manifest = derived_manifest
    if not path.is_file():
        return _evidence_response(
            "NOT_COMPUTED", mode="true_love",
            reason="the completed module has no queryable strict-pair table",
            manifest=manifest, provenance=[str(root / "manifest.json")],
        )
    gene = query.get("gene")
    partner = query.get("partner")
    index_coverage = "all" if catalog == "stable_negative_rank1" else coverage
    rows = _indexed_true_love_rows(
        settings, catalog=catalog, coverage=index_coverage,
        gene=gene, partner=partner, limit=int(query.get("limit", 20)),
    )
    used_index = rows is not None
    if rows is None:
        rows = _filter_pair_rows(_read_csv_records(path), gene, partner)
    if catalog == "stable_negative_rank1":
        rows.sort(key=lambda row: (-float(row.get("bootstrap_reciprocal_stability") or 0), float(row.get("worst_direction_fdr") or 1), -abs(float(row.get("strongest_absolute_correlation") or 0))))
    elif catalog == "negative_r_lt_minus_0_3":
        rows.sort(key=lambda row: (float(row.get("correlation") or 0), str(row.get("gene_a")), str(row.get("gene_b"))))
    else:
        rows.sort(key=lambda row: (float(row.get("reciprocal_rank_sum") or 999), -float(row.get("correlation_a_to_b") or 0), str(row.get("gene_a")), str(row.get("gene_b"))))
    limit = int(query.get("limit", 20))
    return _evidence_response(
        "FOUND" if rows else "NOT_RETAINED", mode="true_love",
        reason=("bounded rows found in the requested completed TLG catalog" if rows else "the completed requested TLG catalog retained no matching pair"),
        gene=gene.strip().upper() if gene else None,
        partner=partner.strip().upper() if partner else None,
        catalog=catalog, coverage=(None if catalog == "stable_negative_rank1" else coverage), rows=rows[:limit],
        summary={"matched_pair_count": len(rows), "returned_count": min(limit, len(rows)), "stability_layer": path == stable_path},
        manifest=selected_manifest,
        provenance=[str(root / "manifest.json"), str(path), *([str(settings.knowledge_root / "depmap-26q1-query-index.sqlite")] if used_index else [])],
    )


def _run_synthetic_lethal_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    root = settings.knowledge_root / "depmap-26q1-full" / "observational_synthetic_lethal_candidates"
    manifest, unavailable = _complete_module(root, mode="synthetic_lethal")
    if unavailable is not None:
        return unavailable
    event = query.get("event")
    path = root / (f"{event}.csv.gz" if event else "pair_evidence_summary.csv.gz")
    if not path.is_file():
        return _evidence_response(
            "NOT_COMPUTED", mode="synthetic_lethal",
            reason="the requested completed evidence-family table is unavailable",
            event=event, manifest=manifest, provenance=[str(root / "manifest.json")],
        )
    source = query.get("source")
    target = query.get("target")
    source = source.strip().upper() if source else None
    target = target.strip().upper() if target else None
    matched = filter_before_limit(
        _iter_csv_records(path),
        lambda row: (source is None or row.get("source_gene") == source)
        and (target is None or row.get("target_gene") == target),
    )
    limit = int(query.get("limit", 20))
    rows, matched_count = bound_after_rank(
        matched,
        key=lambda row: (
            float(row.get("best_fdr") or row.get("fdr") or 1),
            -int(row.get("evidence_family_count") or 0),
            float(row.get("strongest_mean_difference") or row.get("mean_difference") or 0),
        ),
        limit=limit,
    )
    return _evidence_response(
        "FOUND" if rows else "NOT_RETAINED", mode="synthetic_lethal",
        reason=("observational synthetic-lethal candidate evidence found" if rows else "the completed candidate screen retained no matching row"),
        source=source, target=target, event=event, rows=rows,
        summary={"matched_row_count": matched_count, "returned_count": len(rows)},
        manifest=manifest, provenance=[str(root / "manifest.json"), str(path)],
    )


def _catalog_choice(root: Path, filename: str, field: str, requested: str | None) -> tuple[list[dict[str, Any]], str | None]:
    rows = _read_csv_records(root / filename)
    if requested is None:
        return rows, None
    match = next((str(row[field]) for row in rows if str(row.get(field, "")).casefold() == requested.casefold()), None)
    return rows, match


def _run_three_d_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    root = settings.knowledge_root / "depmap-26q1-3d"
    family = query["family"]
    family_root = root / family
    manifest, unavailable = _complete_module(family_root, mode="three_d")
    if unavailable is not None:
        return unavailable
    limit = int(query.get("limit", 20))
    gene = query.get("gene")
    source = query.get("source")
    target = query.get("target")
    symbol = gene.strip().upper() if gene else None
    source = source.strip().upper() if source else None
    target = target.strip().upper() if target else None
    cohort = query.get("cohort")
    contrast = query.get("contrast")
    omic = query.get("omic")
    provenance = [str(family_root / "manifest.json")]

    catalog_specs = {
        "dependency_profiles": ("group_catalog.csv", "group", cohort),
        "differential_dependency": ("contrast_catalog.csv", "contrast", contrast),
        "codependency": ("cohort_catalog.csv", "cohort", cohort),
        "true_love_gene": ("cohort_catalog.csv", "cohort", cohort),
        "omics_dependency": ("modality_catalog.csv", "modality", f"{omic}_dependency" if omic else None),
        "lineage_dependency_enrichment": ("group_catalog.csv", "group", cohort),
    }
    catalog_file, catalog_field, requested = catalog_specs[family]
    catalog_path = family_root / catalog_file
    if not catalog_path.is_file():
        return _evidence_response("NOT_COMPUTED", mode="three_d", reason="the completed 3D family has no catalog", family=family, manifest=manifest, provenance=provenance)
    catalog_rows, selected = _catalog_choice(family_root, catalog_file, catalog_field, requested)
    provenance.append(str(catalog_path))
    selector_present = requested is not None
    if selector_present and selected is None:
        return _evidence_response("NOT_COMPUTED", mode="three_d", reason="the requested 3D cohort, contrast, or modality is not in the completed catalog", family=family, cohort=cohort, contrast=contrast, omic=omic, manifest=manifest, provenance=provenance)
    if not selector_present and not any((symbol, source, target)):
        return _evidence_response("FOUND", mode="three_d", reason="completed 3D analysis-family catalog found", family=family, rows=catalog_rows[:limit], summary={"catalog_count": len(catalog_rows)}, manifest=manifest, provenance=provenance)

    units = [selected] if selected else [str(row[catalog_field]) for row in catalog_rows if row.get("status") == "complete"]
    rows: list[dict[str, Any]] = []
    for unit in units:
        unit_root = family_root / str(unit)
        candidates: list[Path]
        if family == "dependency_profiles":
            candidates = [unit_root / "all_genes.csv.gz"]
        elif family == "differential_dependency":
            candidates = [unit_root / "all_genes.csv.gz"]
        elif family in {"codependency", "true_love_gene"}:
            candidates = [unit_root / "high_confidence_pairs.csv.gz"]
        elif family == "omics_dependency":
            candidates = [unit_root / "significant_associations.csv.gz"]
        else:
            candidates = [unit_root / "significant_enrichment.csv.gz"]
        for path in candidates:
            if not path.is_file() or (_load_manifest(unit_root) or {}).get("status") != "complete":
                continue
            candidates_rows = _read_csv_records(path)
            if family in {"codependency", "true_love_gene"}:
                candidates_rows = _filter_pair_rows(candidates_rows, symbol or source, target)
            elif symbol:
                candidates_rows = [row for row in candidates_rows if str(row.get("gene", "")).upper() == symbol]
            elif source or target:
                candidates_rows = [row for row in candidates_rows if (source is None or str(row.get("feature_gene", "")).upper() == source) and (target is None or str(row.get("target_gene", "")).upper() == target)]
            rows.extend({"unit": unit, **row} for row in candidates_rows)
            provenance.extend([str(unit_root / "manifest.json"), str(path)])
    rows = rows[:limit]
    return _evidence_response(
        "FOUND" if rows else "NOT_RETAINED", mode="three_d",
        reason=("bounded rows from the completed 3D analysis family found" if rows else "the completed 3D analysis family retained no matching row"),
        family=family, cohort=cohort, contrast=contrast, omic=omic,
        gene=symbol, source=source, target=target, rows=rows,
        summary={"searched_unit_count": len(units), "returned_count": len(rows)},
        manifest=manifest, provenance=provenance,
    )


def _evidence_response(
    status_name: str,
    *,
    mode: str,
    reason: str,
    manifest: dict[str, Any] | None = None,
    provenance: list[str] | None = None,
    **identity: Any,
) -> dict[str, Any]:
    if status_name not in EVIDENCE_STATUSES:
        raise ValueError(f"invalid evidence status: {status_name}")
    return {
        "mode": mode,
        "status": status_name,
        "reason": reason,
        **identity,
        "manifest": manifest,
        "provenance": provenance or [],
    }


def _resolve_lineage_module(
    settings: Settings, module: str, lineage: str
) -> tuple[Path | None, dict[str, Any] | None, dict[str, Any] | None]:
    module_root = settings.knowledge_root / "depmap-26q1-full" / module
    if not module_root.is_dir():
        return None, None, _evidence_response(
            "MODULE_UNAVAILABLE",
            mode=module,
            reason="the requested precomputed module is not installed",
            module=module,
            lineage=lineage,
        )
    lineage_root = module_root / _lineage_key(lineage)
    if not lineage_root.is_dir():
        return None, None, _evidence_response(
            "NOT_COMPUTED",
            mode=module,
            reason="no lineage output directory exists for this request",
            module=module,
            lineage=lineage,
            provenance=[str(module_root)],
        )
    manifest = _load_manifest(lineage_root)
    if manifest is None:
        return None, None, _evidence_response(
            "NOT_COMPUTED",
            mode=module,
            reason="the lineage output has no manifest",
            module=module,
            lineage=lineage,
            provenance=[str(lineage_root)],
        )
    if manifest.get("status") != "complete":
        return None, manifest, _evidence_response(
            "INELIGIBLE",
            mode=module,
            reason=f"lineage manifest status is {manifest.get('status', 'unknown')}",
            module=module,
            lineage=lineage,
            manifest=manifest,
            provenance=[str(lineage_root / "manifest.json")],
        )
    return lineage_root, manifest, None


def _source_index(order_path: Path, source: str) -> int | None:
    wanted = source.strip().upper()
    with order_path.open(encoding="utf-8-sig", newline="") as handle:
        for row in csv.DictReader(handle):
            if row.get("symbol", "").strip().upper() == wanted:
                return int(row["source_index"])
    return None


def _source_block(lineage_root: Path, index: int) -> Path | None:
    for path in sorted((lineage_root / "blocks").glob("block_*_*.parquet")):
        match = re.fullmatch(r"block_(\d+)_(\d+)\.parquet", path.name)
        if match and int(match.group(1)) <= index <= int(match.group(2)):
            return path
    return None


def _read_filtered_parquet(
    path: Path,
    filters: list[tuple[str, str, Any]],
) -> list[dict[str, Any]]:
    try:
        import pyarrow.parquet as parquet
    except ImportError:
        raise HTTPException(status_code=500, detail="PyArrow is required for lineage queries")
    if filters:
        return parquet.read_table(path, filters=filters).to_pylist()
    return parquet.read_table(path).to_pylist()


def _bounded_rows(rows: list[dict[str, Any]], value_key: str, limit: int) -> list[dict[str, Any]]:
    rows.sort(
        key=lambda row: abs(float(row.get(value_key) or 0.0)),
        reverse=True,
    )
    return rows[:limit]


def _run_lineage_network_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    family = query["family"]
    lineage = query["lineage"]
    module_root = (
        settings.knowledge_root / "depmap-26q1-full" / "lineage_sparse_networks" / family
    )
    if not module_root.is_dir():
        return _evidence_response(
            "MODULE_UNAVAILABLE", mode="lineage_network",
            reason="the requested lineage network family is not installed",
            family=family, lineage=lineage,
        )
    lineage_root = module_root / _lineage_key(lineage)
    manifest = _load_manifest(lineage_root) if lineage_root.is_dir() else None
    if manifest is None:
        return _evidence_response(
            "NOT_COMPUTED", mode="lineage_network",
            reason="this lineage/family combination was not computed",
            family=family, lineage=lineage, provenance=[str(module_root)],
        )
    if manifest.get("status") != "complete":
        return _evidence_response(
            "INELIGIBLE", mode="lineage_network",
            reason=f"lineage manifest status is {manifest.get('status', 'unknown')}",
            family=family, lineage=lineage, manifest=manifest,
            provenance=[str(lineage_root / "manifest.json")],
        )
    source = query["source"].strip().upper()
    target = query.get("target")
    limit = query.get("limit", 20)
    reciprocal = query.get("reciprocal", False)
    if reciprocal:
        try:
            import pyarrow.dataset as dataset
        except ImportError:
            raise HTTPException(status_code=500, detail="PyArrow is required for lineage queries")
        path = lineage_root / "reciprocal_pairs.parquet"
        if not path.is_file():
            return _evidence_response(
                "NOT_COMPUTED", mode="lineage_network",
                reason="reciprocal-pair output is absent for this eligible lineage",
                family=family, lineage=lineage, source=source, target=target,
                manifest=manifest, provenance=[str(lineage_root / "manifest.json")],
            )
        field = dataset.field
        expr = (field("source_gene") == source) | (field("target_gene") == source)
        if target:
            target = target.strip().upper()
            expr = expr & (
                (field("source_gene") == target) | (field("target_gene") == target)
            )
        rows = dataset.dataset(path, format="parquet").to_table(filter=expr).to_pylist()
        rows = _bounded_rows(rows, "reciprocal_score", limit)
    else:
        order = lineage_root / "source_gene_order.csv"
        index = _source_index(order, source) if order.is_file() else None
        if index is None:
            return _evidence_response(
                "NOT_COMPUTED", mode="lineage_network",
                reason="source gene is absent from the computed source universe",
                family=family, lineage=lineage, source=source, target=target,
                manifest=manifest, provenance=[str(order)],
            )
        path = _source_block(lineage_root, index)
        if path is None:
            return _evidence_response(
                "NOT_COMPUTED", mode="lineage_network",
                reason="the source gene block is absent",
                family=family, lineage=lineage, source=source, target=target,
                manifest=manifest, provenance=[str(lineage_root / "blocks")],
            )
        filters: list[tuple[str, str, Any]] = [("source_gene", "=", source)]
        if target:
            target = target.strip().upper()
            filters.append(("target_gene", "=", target))
        rows = _bounded_rows(_read_filtered_parquet(path, filters), "correlation", limit)
    if not rows:
        return _evidence_response(
            "NOT_RETAINED", mode="lineage_network",
            reason="the eligible pair was tested but is absent from the retained sparse top-K output",
            family=family, lineage=lineage, source=source, target=target,
            reciprocal=reciprocal, manifest=manifest,
            provenance=[str(lineage_root / "manifest.json"), str(path)],
        )
    return _evidence_response(
        "FOUND", mode="lineage_network", reason="bounded precomputed rows found",
        family=family, lineage=lineage, source=source, target=target,
        reciprocal=reciprocal, rows=rows, manifest=manifest,
        provenance=[str(lineage_root / "manifest.json"), str(path)],
    )


def _run_lineage_cnv_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    lineage = query["lineage"]
    root, manifest, gap = _resolve_lineage_module(
        settings, "lineage_cnv_amplification_dependency", lineage
    )
    if gap is not None:
        gap["mode"] = "lineage_cnv"
        gap["source"] = query["source"].strip().upper()
        gap["target"] = (
            query.get("target", "").strip().upper() or None
        )
        return gap
    assert root is not None and manifest is not None
    source = query["source"].strip().upper()
    target = query.get("target")
    limit = query.get("limit", 20)
    order = root / "source_gene_order.csv"
    index = _source_index(order, source) if order.is_file() else None
    if index is None:
        return _evidence_response(
            "INELIGIBLE", mode="lineage_cnv",
            reason="source amplification did not satisfy the manifest's amplified/control thresholds",
            lineage=lineage, source=source, target=target, manifest=manifest,
            provenance=[str(order)],
        )
    path = _source_block(root, index)
    if path is None:
        return _evidence_response(
            "NOT_COMPUTED", mode="lineage_cnv", reason="the source event block is absent",
            lineage=lineage, source=source, target=target, manifest=manifest,
            provenance=[str(root / "blocks")],
        )
    filters: list[tuple[str, str, Any]] = [("source_gene", "=", source)]
    if target:
        target = target.strip().upper()
        filters.append(("target_gene", "=", target))
    rows = _bounded_rows(_read_filtered_parquet(path, filters), "mean_difference", limit)
    if not rows:
        return _evidence_response(
            "NOT_RETAINED", mode="lineage_cnv",
            reason="the eligible pair was tested but is absent from the retained sparse top-K output",
            lineage=lineage, source=source, target=target, manifest=manifest,
            provenance=[str(root / "manifest.json"), str(path)],
        )
    return _evidence_response(
        "FOUND", mode="lineage_cnv", reason="bounded precomputed rows found",
        lineage=lineage, source=source, target=target, rows=rows, manifest=manifest,
        provenance=[str(root / "manifest.json"), str(path)],
    )


def _run_lineage_drug_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    feature = query["omic"]
    lineage = query["lineage"]
    module = settings.knowledge_root / "depmap-26q1-full" / "lineage_prism_associations"
    if not module.is_dir():
        return _evidence_response(
            "MODULE_UNAVAILABLE", mode="lineage_drug",
            reason="lineage PRISM module is not installed", feature=feature, lineage=lineage,
        )
    root = module / feature / _lineage_key(lineage)
    manifest = _load_manifest(root) if root.is_dir() else None
    if manifest is None:
        return _evidence_response(
            "NOT_COMPUTED", mode="lineage_drug",
            reason="this lineage/omic combination was not computed",
            feature=feature, lineage=lineage, provenance=[str(module / feature)],
        )
    if manifest.get("status") != "complete":
        return _evidence_response(
            "INELIGIBLE", mode="lineage_drug",
            reason=f"lineage manifest status is {manifest.get('status', 'unknown')}",
            feature=feature, lineage=lineage, manifest=manifest,
            provenance=[str(root / "manifest.json")],
        )
    path = root / "associations.parquet"
    filters: list[tuple[str, str, Any]] = []
    metadata = _read_filtered_parquet(root / "drug_metadata.parquet", [])
    drug_names = {
        str(row.get("CompoundID")): row.get("CompoundName")
        for row in metadata
        if row.get("CompoundID") is not None
    }
    target = query.get("target")
    if target:
        target = target.strip().upper()
        filters.append(("gene", "=", target))
    drug = query.get("drug")
    drug_ids: list[str] = []
    if drug:
        wanted = drug.strip().upper()
        drug_ids = [
            row.get("CompoundID")
            for row in metadata
            if wanted in {
                str(row.get("CompoundID", "")).upper(),
                str(row.get("CompoundName", "")).upper(),
            }
        ]
        if not drug_ids:
            return _evidence_response(
                "NOT_COMPUTED", mode="lineage_drug",
                reason="drug is absent from this lineage PRISM index",
                feature=feature, lineage=lineage, drug=drug, target=target,
                manifest=manifest, provenance=[str(root / "drug_metadata.parquet")],
            )
        filters.append(("drug_id", "in", drug_ids))
    rows = _bounded_rows(
        _read_filtered_parquet(path, filters), "pearson_r", query.get("limit", 20)
    )
    for row in rows:
        row["drug_name"] = drug_names.get(str(row.get("drug_id")))
    if not rows:
        return _evidence_response(
            "NOT_RETAINED", mode="lineage_drug",
            reason="the eligible association is absent from the retained sparse top-K output",
            feature=feature, lineage=lineage, drug=drug, target=target,
            manifest=manifest, provenance=[str(root / "manifest.json"), str(path)],
        )
    return _evidence_response(
        "FOUND", mode="lineage_drug", reason="bounded precomputed rows found",
        feature=feature, lineage=lineage, drug=drug, target=target,
        rows=rows, manifest=manifest,
        provenance=[str(root / "manifest.json"), str(path)],
    )


def _run_enrichment_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    lineage = query["lineage"]
    root, manifest, gap = _resolve_lineage_module(settings, "lineage_gene_enrichment", lineage)
    if gap is not None:
        gap["mode"] = "enrichment"
        gap["source"] = query["source"].strip().upper()
        gap["collection"] = query.get("collection")
        gap["term"] = query.get("term")
        return gap
    assert root is not None and manifest is not None
    source = query["source"].strip().upper()
    rows: list[dict[str, Any]] = []
    provenance = [str(root / "manifest.json")]
    filters: list[tuple[str, str, Any]] = [("source_gene", "=", source)]
    if query.get("collection"):
        filters.append(("collection", "=", query["collection"]))
    if query.get("term"):
        filters.append(("term", "=", query["term"]))
    for path in sorted((root / "blocks").glob("block_*_*.parquet")):
        hit = _read_filtered_parquet(path, filters)
        if hit:
            rows.extend(hit)
            provenance.append(str(path))
            break
    rows = _bounded_rows(rows, "enrichment_z", query.get("limit", 20))
    if not rows:
        return _evidence_response(
            "NOT_RETAINED", mode="enrichment",
            reason="no retained enrichment row matched this eligible gene/lineage request",
            lineage=lineage, source=source, collection=query.get("collection"),
            term=query.get("term"), manifest=manifest, provenance=provenance,
        )
    return _evidence_response(
        "FOUND", mode="enrichment", reason="bounded precomputed rows found",
        lineage=lineage, source=source, collection=query.get("collection"),
        term=query.get("term"), rows=rows, manifest=manifest, provenance=provenance,
    )


def _top_precomputed_rows(
    paths: list[Path],
    *,
    columns: list[str],
    filters: list[tuple[str, str, Any]],
    value_key: str,
    limit: int,
    identity: Callable[[dict[str, Any]], Any],
) -> tuple[list[dict[str, Any]], int, list[str]]:
    """Select top absolute values from retained Parquet rows without recomputing stats."""

    try:
        import pyarrow.compute as compute
        import pyarrow.parquet as parquet
    except ImportError:
        raise HTTPException(status_code=500, detail="PyArrow is required for direction discovery")
    candidates: list[dict[str, Any]] = []
    eligible_row_count = 0
    provenance: list[str] = []
    local_limit = max(limit * 8, 100)
    for path in paths:
        table = parquet.read_table(path, columns=columns, filters=filters)
        eligible_row_count += table.num_rows
        if table.num_rows == 0:
            continue
        table = table.append_column(
            "_selection_score", compute.abs(table[value_key])
        ).sort_by([("_selection_score", "descending")])
        candidates.extend(table.slice(0, local_limit).to_pylist())
        provenance.append(str(path))
    candidates.sort(
        key=lambda row: float(row.get("_selection_score") or 0.0), reverse=True
    )
    selected: list[dict[str, Any]] = []
    seen: set[Any] = set()
    for row in candidates:
        key = identity(row)
        if key is None or key in seen:
            continue
        seen.add(key)
        row["selection_score"] = row.pop("_selection_score", None)
        selected.append(row)
        if len(selected) >= limit:
            break
    return selected, eligible_row_count, provenance


def _pair_identity(row: dict[str, Any]) -> tuple[str, str] | None:
    source = str(row.get("source_gene") or "").strip().upper()
    target = str(row.get("target_gene") or "").strip().upper()
    if not source or not target or source == target:
        return None
    return tuple(sorted((source, target)))


def _directed_pair_identity(row: dict[str, Any]) -> tuple[str, str] | None:
    source = str(row.get("source_gene") or "").strip().upper()
    target = str(row.get("target_gene") or "").strip().upper()
    if not source or not target:
        return None
    return source, target


def _direction_section(
    *,
    label: str,
    status_name: str,
    rows: list[dict[str, Any]],
    eligible_row_count: int,
    metric: str,
    interpretation: str,
    provenance: list[str],
    qc_note: str | None = None,
) -> dict[str, Any]:
    return {
        "label": label,
        "status": status_name,
        "eligible_retained_row_count": eligible_row_count,
        "returned_candidate_count": len(rows),
        "metric": metric,
        "interpretation": interpretation,
        "selection_scope": "top absolute value among already retained rows passing fixed filters",
        "qc_note": qc_note,
        "rows": rows,
        "provenance": provenance,
    }


def _run_lineage_directions_query(
    settings: Settings, query: dict[str, Any]
) -> dict[str, Any]:
    lineage = query["lineage"]
    limit = query.get("limit", 20)
    full = settings.knowledge_root / "depmap-26q1-full"
    sections: list[dict[str, Any]] = []

    network_minima: dict[str, int] = {}
    for family in ("effect_correlation", "expression_correlation", "expression_dependency"):
        root = full / "lineage_sparse_networks" / family / _lineage_key(lineage)
        manifest = _load_manifest(root) if root.is_dir() else None
        reciprocal = family != "expression_dependency"
        paths = (
            [root / "reciprocal_pairs.parquet"]
            if reciprocal else sorted((root / "blocks").glob("*.parquet"))
        )
        metric = "reciprocal_score" if reciprocal else "correlation"
        interpretation = (
            "signed reciprocal within-lineage correlation; candidate network edge, not synthetic-lethality proof"
            if reciprocal else
            "expression feature versus CRISPR dependency correlation; candidate biomarker, not causal"
        )
        section = _direction_section(
            label=family, status_name="NOT_COMPUTED", rows=[],
            eligible_row_count=0, metric=metric, interpretation=interpretation,
            provenance=[str(root / "manifest.json")],
        )
        sections.append(section)
        if manifest is None or manifest.get("status") != "complete":
            section["reason"] = "a complete network manifest is unavailable"
            continue
        min_n = manifest.get("min_n")
        # bool is an int subclass, but is not a scientific sample minimum.
        if type(min_n) is not int or min_n <= 0:
            section["status"] = "MODULE_UNAVAILABLE"
            section["reason"] = "network manifest.min_n must be a positive integer; no sample threshold was assumed"
            continue
        network_minima[family] = min_n
        section["selection_filters"] = {
            "fdr_max": 0.05,
            "pair_n_min": min_n,
            "pair_n_min_source": "manifest.min_n",
        }
        cohort_n = manifest.get("lineage_sample_n")
        if type(cohort_n) is int and cohort_n < min_n:
            section["status"] = "INELIGIBLE"
            section["reason"] = "manifest.lineage_sample_n is below manifest.min_n"
            continue
        if not paths or any(not path.is_file() for path in paths):
            section["reason"] = "retained network files are unavailable"
            continue
        columns = [
            "family", "lineage", "source_gene", "target_gene", "correlation",
            "pair_n", "p_value", "fdr",
        ] + (
            ["direction", "reverse_correlation", "reciprocal_rank_max", "reciprocal_score"]
            if reciprocal else ["rank_absolute"]
        )
        rows, count, provenance = _top_precomputed_rows(
            paths, columns=columns,
            filters=[("fdr", "<=", 0.05), ("pair_n", ">=", min_n)],
            value_key=metric, limit=limit,
            identity=_pair_identity if reciprocal else _directed_pair_identity,
        )
        if reciprocal:
            for row in rows:
                if abs(float(row.get("correlation") or 0.0)) >= 0.999:
                    row["qc_flag"] = "near_perfect_correlation_requires_variance_and_identifier_review"
        section.update(
            status="FOUND" if rows else "NOT_RETAINED",
            eligible_retained_row_count=count,
            returned_candidate_count=len(rows),
            rows=rows,
            provenance=[str(root / "manifest.json"), *provenance],
            reason=None if rows else "no retained rows pass the module sample minimum and FDR filter; this is not a biological negative",
            qc_note=(
                "Near-perfect expression correlations may reflect low variance, duplicated features, or identifier artifacts."
                if family == "expression_correlation" else None
            ),
        )

    cnv_root = full / "lineage_cnv_amplification_dependency" / _lineage_key(lineage)
    cnv_paths = sorted((cnv_root / "blocks").glob("*.parquet"))
    rows, count, provenance = _top_precomputed_rows(
        cnv_paths,
        columns=[
            "family", "lineage", "source_gene", "target_gene", "mean_difference",
            "amplified_mean_effect", "wildtype_mean_effect", "amplified_n",
            "wildtype_n", "p_value", "fdr", "direction",
        ],
        filters=[
            ("fdr", "<=", 0.05), ("amplified_n", ">=", 5),
            ("wildtype_n", ">=", 10),
        ],
        value_key="mean_difference", limit=limit, identity=_directed_pair_identity,
    )
    sections.append(
        _direction_section(
            label="cnv_amplification_dependency",
            status_name="FOUND" if rows else "NOT_RETAINED", rows=rows,
            eligible_row_count=count, metric="mean_difference",
            interpretation="amplified-group minus control-group dependency; negative indicates stronger dependency in amplified models",
            provenance=[str(cnv_root / "manifest.json"), *provenance],
        )
    )

    enrichment_root = full / "lineage_gene_enrichment" / _lineage_key(lineage)
    enrichment_paths = sorted((enrichment_root / "blocks").glob("*.parquet"))
    rows, count, provenance = _top_precomputed_rows(
        enrichment_paths,
        columns=[
            "source_gene", "collection", "term", "enrichment_z", "p_value",
            "fdr", "gene_set_collection", "lineage",
        ],
        filters=[("fdr", "<=", 0.05)], value_key="enrichment_z", limit=limit,
        identity=lambda row: (
            row.get("source_gene"), row.get("collection"), row.get("term")
        ),
    )
    sections.append(
        _direction_section(
            label="pathway_tf_enrichment", status_name="FOUND" if rows else "NOT_RETAINED",
            rows=rows, eligible_row_count=count, metric="enrichment_z",
            interpretation="signed enrichment over expression-to-dependency correlations; hypothesis-generating pathway/TF context",
            provenance=[str(enrichment_root / "manifest.json"), *provenance],
        )
    )

    for feature in ("effect", "expression", "cnv"):
        drug_root = full / "lineage_prism_associations" / feature / _lineage_key(lineage)
        path = drug_root / "associations.parquet"
        metadata_path = drug_root / "drug_metadata.parquet"
        paths = [path] if path.is_file() else []
        rows, count, provenance = _top_precomputed_rows(
            paths,
            columns=[
                "lineage", "feature", "drug_id", "gene", "n", "pearson_r",
                "p_value", "fdr_within_drug", "retained_by",
            ],
            filters=[("fdr_within_drug", "<=", 0.05), ("n", ">=", 10)],
            value_key="pearson_r", limit=limit,
            identity=lambda row: (row.get("drug_id"), row.get("gene")),
        )
        if rows and metadata_path.is_file():
            metadata = _read_filtered_parquet(metadata_path, [])
            names = {
                str(item.get("CompoundID")): item.get("CompoundName")
                for item in metadata
            }
            for row in rows:
                row["drug_name"] = names.get(str(row.get("drug_id")))
        sections.append(
            _direction_section(
                label=f"prism_{feature}", status_name="FOUND" if rows else "NOT_RETAINED",
                rows=rows, eligible_row_count=count, metric="pearson_r_vs_prism_auc",
                interpretation="positive means higher feature associates with higher AUC (lower sensitivity); FDR is within drug",
                provenance=[str(drug_root / "manifest.json"), str(metadata_path), *provenance],
            )
        )

    gene_support: dict[str, dict[str, Any]] = {}
    for section in sections:
        label = section["label"]
        for row in section["rows"]:
            genes: list[tuple[str, str]] = []
            if row.get("source_gene"):
                genes.append((str(row["source_gene"]), "source"))
            if row.get("target_gene"):
                genes.append((str(row["target_gene"]), "target"))
            if row.get("gene"):
                genes.append((str(row["gene"]), "drug_feature"))
            for gene, role in genes:
                entry = gene_support.setdefault(
                    gene.upper(), {"gene": gene.upper(), "families": set(), "roles": set()}
                )
                entry["families"].add(label)
                entry["roles"].add(role)
    convergence = [
        {
            "gene": item["gene"],
            "family_count": len(item["families"]),
            "families": sorted(item["families"]),
            "roles": sorted(item["roles"]),
        }
        for item in gene_support.values()
    ]
    convergence.sort(key=lambda item: (-item["family_count"], item["gene"]))

    topic_types = {
        "effect_correlation": "reciprocal_dependency_pair",
        "expression_correlation": "expression_network_pair_qc_sensitive",
        "expression_dependency": "expression_dependency_biomarker",
        "cnv_amplification_dependency": "cnv_dependency_contrast",
        "pathway_tf_enrichment": "pathway_or_tf_context",
        "prism_effect": "dependency_drug_response_biomarker",
        "prism_expression": "expression_drug_response_biomarker",
        "prism_cnv": "cnv_drug_response_biomarker",
    }
    topic_candidates: list[dict[str, Any]] = []
    for item in convergence:
        if item["family_count"] < 2 or len(topic_candidates) >= limit:
            continue
        topic_candidates.append(
            {
                "candidate_id": f"multi_evidence_gene:{item['gene']}",
                "topic_type": "multi_family_gene_followup",
                "priority_tier": 1,
                "anchors": {"gene": item["gene"]},
                "basis": "gene appears in multiple independently ranked family shortlists",
                "families": item["families"],
                "caveat": "family recurrence is an unweighted retrieval count, not a combined significance score",
            }
        )
    max_rank = max((len(section["rows"]) for section in sections), default=0)
    for rank in range(max_rank):
        for section in sections:
            if len(topic_candidates) >= limit or rank >= len(section["rows"]):
                continue
            row = section["rows"][rank]
            anchors = {
                key: row[key]
                for key in (
                    "source_gene", "target_gene", "gene", "drug_id", "drug_name",
                    "collection", "term",
                )
                if row.get(key) is not None
            }
            identity_text = "|".join(str(value) for value in anchors.values())
            topic_candidates.append(
                {
                    "candidate_id": f"{section['label']}:{identity_text}",
                    "topic_type": topic_types[section["label"]],
                    "priority_tier": 2,
                    "family": section["label"],
                    "rank_within_family": rank + 1,
                    "anchors": anchors,
                    "metric": section["metric"],
                    "observed_value": row.get(
                        {
                            "effect_correlation": "reciprocal_score",
                            "expression_correlation": "reciprocal_score",
                            "expression_dependency": "correlation",
                            "cnv_amplification_dependency": "mean_difference",
                            "pathway_tf_enrichment": "enrichment_z",
                            "prism_effect": "pearson_r",
                            "prism_expression": "pearson_r",
                            "prism_cnv": "pearson_r",
                        }[section["label"]]
                    ),
                    "basis": "balanced round-robin selection from a fixed-filter family shortlist",
                    "qc_flag": row.get("qc_flag"),
                }
            )
        if len(topic_candidates) >= limit:
            break

    return {
        "mode": "lineage_directions",
        "status": "FOUND" if any(section["rows"] for section in sections) else "NOT_RETAINED",
        "release": settings.release,
        "lineage": lineage,
        "selection_policy": {
            "network_and_enrichment_fdr_max": 0.05,
            # Legacy scalar summarizes the lowest declared minimum; consumers
            # needing the actual filter should use the per-family/section fields.
            "network_pair_n_min": min(network_minima.values(), default=None),
            "network_pair_n_min_by_family": network_minima,
            "network_sample_policy": "each network uses its own manifest.min_n",
            "cnv_fdr_max": 0.05,
            "cnv_amplified_n_min": 5,
            "cnv_control_n_min": 10,
            "prism_within_drug_fdr_max": 0.05,
            "prism_n_min": 10,
            "ranking": "absolute value within each statistically distinct family",
            "cross_family_policy": "unweighted family-presence count among returned shortlists; metrics are never numerically combined",
        },
        "sections": sections,
        "cross_family_gene_mentions": convergence[:limit],
        "topic_candidates": topic_candidates,
        "topic_candidate_policy": (
            "multi-family mentions first, then balanced round-robin across statistically distinct families; "
            "no cross-metric numeric score is invented"
        ),
        "limitations": [
            "Candidates are selected from retained sparse outputs, not raw dense matrices.",
            "Top rank is hypothesis-generating and is not proof of causality, novelty, druggability, or clinical actionability.",
            "Expression-correlation near-perfect edges require variance and identifier QC.",
            "Literature and clinical validation are separate downstream steps.",
        ],
        "new_analysis_started": False,
    }


def _lineage_catalog_item(label: str, module_root: Path, lineage: str) -> dict[str, Any]:
    if not module_root.is_dir():
        return {
            "label": label,
            "status": "MODULE_UNAVAILABLE",
            "reason": "the requested precomputed module is not installed",
            "manifest": None,
            "provenance": [str(module_root)],
        }
    lineage_root = module_root / _lineage_key(lineage)
    manifest = _load_manifest(lineage_root) if lineage_root.is_dir() else None
    if manifest is None:
        return {
            "label": label,
            "status": "NOT_COMPUTED",
            "reason": "no lineage output manifest exists for this module",
            "manifest": None,
            "provenance": [str(module_root)],
        }
    complete = manifest.get("status") == "complete"
    return {
        "label": label,
        "status": "FOUND" if complete else "INELIGIBLE",
        "reason": (
            "complete lineage module is available"
            if complete
            else f"lineage manifest status is {manifest.get('status', 'unknown')}"
        ),
        "manifest": manifest,
        "provenance": [str(lineage_root / "manifest.json")],
    }


def _tcga_module_root(settings: Settings) -> Path:
    return settings.knowledge_root / "depmap-26q1-tcga"


def _tcga_project_catalog(settings: Settings) -> list[dict[str, Any]]:
    path = _tcga_module_root(settings) / "project_catalog.csv"
    if not path.is_file():
        return []
    with path.open(encoding="utf-8-sig", newline="") as handle:
        return list(csv.DictReader(handle))


def _run_tcga_expression_survival_query(
    settings: Settings, query: dict[str, Any]
) -> dict[str, Any]:
    """Return bounded, precomputed TCGA expression-survival evidence for one gene."""

    module_root = _tcga_module_root(settings)
    qa_path = module_root / "qa.json"
    if not qa_path.is_file():
        return _evidence_response(
            "MODULE_UNAVAILABLE",
            mode="tcga_expression_survival",
            reason="the TCGA expression-survival bridge is not installed",
            gene=query["gene"].strip().upper(),
            endpoint=(query.get("endpoint") or "OS").upper(),
            provenance=[str(module_root)],
        )
    with qa_path.open(encoding="utf-8-sig") as handle:
        qa = json.load(handle)
    if qa.get("status") != "PASS":
        return _evidence_response(
            "INELIGIBLE",
            mode="tcga_expression_survival",
            reason="the installed TCGA expression-survival bridge has not passed QA",
            gene=query["gene"].strip().upper(),
            endpoint=(query.get("endpoint") or "OS").upper(),
            manifest=qa,
            provenance=[str(qa_path)],
        )

    gene = query["gene"].strip().upper()
    endpoint = (query.get("endpoint") or "OS").upper()
    requested_project = query.get("project")
    requested_lineage = query.get("lineage")
    limit = query.get("limit", 100)
    catalog = _tcga_project_catalog(settings)
    selected = [
        row for row in catalog
        if (requested_project is None or row.get("tcga_project") == requested_project)
        and (requested_lineage is None or row.get("depmap_lineage") == requested_lineage)
        and row.get("status") == "complete"
    ]
    if not selected:
        return _evidence_response(
            "NOT_COMPUTED",
            mode="tcga_expression_survival",
            reason="no completed TCGA project matches the requested project or DepMap lineage",
            gene=gene,
            project=requested_project,
            lineage=requested_lineage,
            endpoint=endpoint,
            manifest=qa,
            provenance=[str(module_root / "project_catalog.csv"), str(qa_path)],
        )

    prefix = f"expression_{endpoint.lower()}"
    columns = [
        "symbol", "hgnc_id", "entrez_id", "ensembl_gene_id", "tcga_project",
        "depmap_lineage", "expression_available", "expression_n",
        "expression_mapping_basis", "expression_median_log2_tpm",
        f"{prefix}_n", f"{prefix}_events",
        f"{prefix}_score_z", f"{prefix}_p_value", f"{prefix}_fdr",
    ]
    rows: list[dict[str, Any]] = []
    provenance = [str(qa_path), str(module_root / "project_catalog.csv")]
    missing_projects: list[str] = []
    association_status_counts: dict[str, int] = {}
    thresholds: set[tuple[int, int]] = set()
    for item in selected:
        project = item["tcga_project"]
        project_root = module_root / "projects" / project
        path = project_root / "gene_associations.parquet"
        manifest_path = project_root / "manifest.json"
        if not path.is_file():
            missing_projects.append(project)
            continue
        project_manifest: dict[str, Any] = {}
        if manifest_path.is_file():
            with manifest_path.open(encoding="utf-8-sig") as handle:
                project_manifest = json.load(handle)
        min_n = int(project_manifest.get("min_n", 30))
        min_events = int(project_manifest.get("min_events", 10))
        thresholds.add((min_n, min_events))
        project_rows = _read_filtered_parquet(path, [("symbol", "=", gene)])
        for row in project_rows:
            if not row.get("expression_available"):
                continue
            result_row = {key: row.get(key) for key in columns}
            cohort_n = row.get(f"{prefix}_n")
            events = row.get(f"{prefix}_events")
            score = row.get(f"{prefix}_score_z")
            if cohort_n is None or events is None:
                association_status = "NOT_COMPUTED"
            elif cohort_n < min_n or events < min_events or score is None:
                association_status = "INELIGIBLE"
            else:
                association_status = "FOUND"
            result_row["association_status"] = association_status
            result_row["min_n"] = min_n
            result_row["min_events"] = min_events
            association_status_counts[association_status] = (
                association_status_counts.get(association_status, 0) + 1
            )
            rows.append(result_row)
        provenance.extend((str(manifest_path), str(path)))
        if len(rows) >= limit:
            break
    rows = rows[:limit]
    if not rows:
        return _evidence_response(
            "NOT_COMPUTED",
            mode="tcga_expression_survival",
            reason="the gene is absent from the matched TCGA expression universe",
            gene=gene,
            project=requested_project,
            lineage=requested_lineage,
            endpoint=endpoint,
            manifest=qa,
            provenance=provenance,
        )
    response_status = (
        "FOUND" if association_status_counts.get("FOUND", 0) else "INELIGIBLE"
    )
    return _evidence_response(
        response_status,
        mode="tcga_expression_survival",
        reason=(
            "bounded precomputed TCGA expression-survival rows found"
            if response_status == "FOUND"
            else "expression is available, but no selected project has a testable survival association"
        ),
        gene=gene,
        project=requested_project,
        lineage=requested_lineage,
        endpoint=endpoint,
        rows=rows,
        summary={
            "matched_project_count": len(rows),
            "selected_project_count": len(selected),
            "missing_project_outputs": missing_projects,
            "target_gene_count": qa.get("target_gene_count"),
            "association_status_counts": association_status_counts,
        },
        manifest={
            "release": qa.get("release"),
            "method": "Breslow univariate Cox score test at beta=0",
            "multiple_testing": "BH FDR within TCGA project and survival endpoint",
            "endpoint": endpoint,
            "expression_scale": "log2(TPM+1)",
            "eligibility_thresholds": [
                {"min_n": min_n, "min_events": min_events}
                for min_n, min_events in sorted(thresholds)
            ],
        },
        provenance=provenance,
    )


def _run_lineage_catalog_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    lineage = query["lineage"]
    full = settings.knowledge_root / "depmap-26q1-full"
    specs = [
        (
            f"network:{family}",
            full / "lineage_sparse_networks" / family,
        )
        for family in sorted(LINEAGE_NETWORK_FAMILIES)
    ]
    specs.extend(
        [
            ("cnv:amplification_dependency", full / "lineage_cnv_amplification_dependency"),
            *[
                (f"drug:{omic}", full / "lineage_prism_associations" / omic)
                for omic in sorted(DRUG_OMICS)
            ],
            ("enrichment:pathway_and_tf", full / "lineage_gene_enrichment"),
        ]
    )
    modules = [_lineage_catalog_item(label, root, lineage) for label, root in specs]
    subtype_root = full / "subtype_dependency"
    subtype_manifest = _load_manifest(subtype_root)
    subtype_catalog = subtype_root / "contrast_catalog.csv"
    subtype_rows = []
    if subtype_manifest and subtype_manifest.get("status") == "complete" and subtype_catalog.is_file():
        subtype_rows = [
            row for row in _read_csv_records(subtype_catalog)
            if row.get("eligible") is True and row.get("lineage") == lineage
        ]
    modules.append({
        "label": "subtype:dependency",
        "status": "FOUND" if subtype_rows else (
            "NOT_COMPUTED" if subtype_root.is_dir() else "MODULE_UNAVAILABLE"
        ),
        "reason": (
            "eligible completed subtype contrasts are available"
            if subtype_rows else "no eligible completed subtype contrast matches this lineage"
        ),
        "contrast_count": len(subtype_rows),
        "contrasts": [row.get("contrast_id") for row in subtype_rows],
        "manifest": subtype_manifest,
        "provenance": [str(subtype_root / "manifest.json"), str(subtype_catalog)],
    })
    tcga_projects = [
        row for row in _tcga_project_catalog(settings)
        if row.get("depmap_lineage") == lineage and row.get("status") == "complete"
    ]
    modules.append({
        "label": "tcga:expression_survival",
        "status": "FOUND" if tcga_projects else "NOT_COMPUTED",
        "reason": (
            "completed TCGA expression-survival projects are available"
            if tcga_projects else "no completed TCGA project maps to this DepMap lineage"
        ),
        "projects": [row.get("tcga_project") for row in tcga_projects],
        "manifest": {"project_count": len(tcga_projects)},
        "provenance": [str(_tcga_module_root(settings) / "project_catalog.csv")],
    })
    available = sum(item["status"] == "FOUND" for item in modules)
    return {
        "mode": "lineage_catalog",
        "state": "precomputed_query" if available else "coverage_gap",
        "release": settings.release,
        "lineage": lineage,
        "modules": modules,
        "summary": {
            "module_count": len(modules),
            "available_module_count": available,
            "coverage_gap_count": len(modules) - available,
        },
        "scope": "lineage_availability_only",
        "new_analysis_started": False,
    }


def _common_essential_labels(settings: Settings, source: str) -> set[str] | None:
    if source != "depmap_26q1":
        return None
    path = settings.knowledge_root / "depmap-26q1-core" / "common_essential_genes.csv"
    if not path.is_file():
        return None
    labels: set[str] = set()
    for row in _iter_csv_records(path):
        symbol = row.get("symbol") or row.get("gene") or row.get("Gene") or row.get("gene_symbol")
        if symbol:
            labels.add(str(symbol).strip().upper())
    return labels


def _lineage_dependency_root(settings: Settings) -> Path:
    return settings.knowledge_root / "depmap-26q1-core" / "lineage_dependency_tests"


def _lineage_dependency_parquets(root: Path, lineage: str | None = None) -> list[Path]:
    if lineage:
        key = _lineage_key(lineage)
        return sorted(root.glob(f"[0-9]*_{key}.parquet"))
    return sorted(root.glob("[0-9]*_*.parquet"))


def _selectivity_retained(row: dict[str, Any], ranking: str) -> bool:
    if ranking == "mean_dependency":
        return row.get("effect_mean_lineage") is not None
    fdr = row.get("fdr_lineage_more_dependent")
    delta = row.get("effect_mean_difference")
    return (
        fdr is not None
        and delta is not None
        and float(fdr) <= 0.05
        and float(delta) < 0
    )


def _exact_lineage_gene_state(
    row: dict[str, Any],
    *,
    ranking: str,
    labels: set[str] | None,
    source: str,
    exclude: bool,
) -> tuple[dict[str, Any], str, dict[str, Any]]:
    tested = str(row.get("test_status") or "").lower() == "tested"
    retained = tested and _selectivity_retained(row, ranking)
    annotated, meta = annotate_common_essential(
        [row], labels=labels, source=source, exclude=False
    )
    out = annotated[0]
    if exclude and meta["annotation_status"] == "AVAILABLE" and out["is_common_essential"]:
        retained = False
    status = classify_tested_entity(in_table=True, tested=tested, retained=retained)
    return out, status, meta


def _run_lineage_dependency_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    lineage = _canonical_lineage_label(query["lineage"])
    ranking = query.get("ranking") or "selective"
    gene = str(query["gene"]).strip().upper() if query.get("gene") else None
    exclude = bool(query.get("exclude_common_essential"))
    source = query.get("common_essential_source") or "depmap_26q1"
    limit = min(int(query.get("limit") or 20), 100)
    root = _lineage_dependency_root(settings)
    coverage = classify_coverage(module_present=root.is_dir())
    if coverage:
        return _evidence_response(
            coverage, mode="lineage_dependency", lineage=lineage, ranking=ranking,
            reason="the precomputed lineage dependency-test module is not installed",
            provenance=[str(root)],
        )
    paths = _lineage_dependency_parquets(root, lineage)
    manifest_path = root / "manifest.json"
    manifest = _load_manifest(root)
    if not paths:
        return _evidence_response(
            "NOT_COMPUTED", mode="lineage_dependency", lineage=lineage, ranking=ranking,
            reason="no completed lineage-vs-rest dependency table matches this lineage",
            manifest=manifest, provenance=[str(manifest_path)],
        )
    path = paths[0]
    table = _read_parquet_records(path)
    labels = _common_essential_labels(settings, source)
    if gene:
        hits = filter_before_limit(
            table, lambda row: str(row.get("symbol") or "").upper() == gene
        )
        if not hits:
            status = classify_tested_entity(in_table=False, tested=False, retained=None)
            return _evidence_response(
                status, mode="lineage_dependency", lineage=lineage, ranking=ranking, gene=gene,
                reason="the gene is absent from the completed lineage test universe; top-N absence is not used",
                rejection_reason=status, rows=[], returned_count=0,
                exclude_common_essential_requested=exclude,
                common_essential_annotation_status=(
                    "AVAILABLE" if labels is not None else "ANNOTATION_UNAVAILABLE"
                ),
                housekeeping_filter_applied=False,
                manifest=manifest, provenance=[str(manifest_path), str(path)],
            )
        row = hits[0]
        out, status, meta = _exact_lineage_gene_state(
            row, ranking=ranking, labels=labels, source=source, exclude=exclude
        )
        return _evidence_response(
            status, mode="lineage_dependency", lineage=lineage, ranking=ranking, gene=gene,
            reason="exact gene row from the completed lineage-vs-rest table",
            rejection_reason=None if status == "FOUND" else status,
            rows=[out], returned_count=1, matched_row_count=1,
            tested_gene_count=sum(1 for item in table if str(item.get("test_status") or "").lower() == "tested"),
            rank_more_dependent=out.get("rank_more_dependent"),
            exclude_common_essential_requested=exclude,
            common_essential_filter_applied=bool(exclude and out.get("is_common_essential") and labels is not None),
            common_essential_source=source,
            common_essential_annotation_status=meta["annotation_status"],
            housekeeping_filter_applied=False,
            manifest=manifest, provenance=[str(manifest_path), str(path)],
        )
    tested_rows = [
        row for row in table
        if str(row.get("test_status") or "").lower() == "tested"
        and row.get("effect_mean_lineage") is not None
    ]
    candidates = [row for row in tested_rows if _selectivity_retained(row, ranking)]
    annotated, meta = annotate_common_essential(
        candidates, labels=labels, source=source, exclude=exclude
    )
    if exclude and labels is None:
        annotated, meta = annotate_common_essential(
            candidates, labels=None, source=source, exclude=False
        )
        meta = {**meta, "filter_applied": False}
    page, matched = bound_after_rank(
        annotated,
        key=lambda row: (
            float(row.get("rank_more_dependent") or 10**9),
            float(row.get("effect_mean_difference") or 0),
        ),
        limit=limit,
    )
    return _evidence_response(
        "FOUND" if page else "NOT_RETAINED",
        mode="lineage_dependency", lineage=lineage, ranking=ranking,
        reason="bounded rows selected from the completed precomputed lineage dependency test",
        rows=page, returned_count=len(page), matched_row_count=matched,
        exclude_common_essential_requested=exclude,
        common_essential_filter_applied=meta["filter_applied"],
        common_essential_source=source,
        common_essential_annotation_status=meta["annotation_status"],
        common_essential_removed_count=meta["removed_count"],
        housekeeping_filter_applied=False,
        summary={
            "tested_gene_count": len(tested_rows),
            "eligible_before_common_essential_filter": meta["before_count"],
            "eligible_after_common_essential_filter": meta["after_count"],
        },
        manifest=manifest, provenance=[str(manifest_path), str(path)],
    )


def _run_pan_cancer_dependency_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    ranking = query.get("ranking") or "selective"
    gene = str(query["gene"]).strip().upper() if query.get("gene") else None
    exclude = bool(query.get("exclude_common_essential"))
    source = query.get("common_essential_source") or "depmap_26q1"
    limit = min(int(query.get("limit") or 5), 20)
    root = _lineage_dependency_root(settings)
    if not root.is_dir():
        return _evidence_response(
            "MODULE_UNAVAILABLE", mode="pan_cancer_dependency", ranking=ranking,
            reason="the precomputed lineage dependency-test module is not installed",
        )
    paths = _lineage_dependency_parquets(root)
    if not paths:
        return _evidence_response(
            "NOT_COMPUTED", mode="pan_cancer_dependency", ranking=ranking,
            reason="no completed lineage-vs-rest dependency tables are indexed",
        )
    labels = _common_essential_labels(settings, source)
    manifest = _load_manifest(root)
    lineages: list[dict[str, Any]] = []
    for path in paths:
        table = _read_parquet_records(path)
        lineage = str((table[0].get("lineage") if table else "") or "")
        if gene:
            hits = filter_before_limit(
                table, lambda row, symbol=gene: str(row.get("symbol") or "").upper() == symbol
            )
            if not hits:
                lineages.append({
                    "lineage": lineage or path.stem,
                    "association_status": "NOT_TESTED",
                    "rows": [],
                })
                continue
            row = hits[0]
            out, status, _meta = _exact_lineage_gene_state(
                row, ranking=ranking, labels=labels, source=source, exclude=exclude
            )
            lineages.append({
                "lineage": lineage or path.stem,
                "association_status": status,
                "rows": [out],
            })
            continue
        tested_rows = [
            row for row in table
            if str(row.get("test_status") or "").lower() == "tested"
        ]
        candidates = [row for row in tested_rows if _selectivity_retained(row, ranking)]
        annotated, meta = annotate_common_essential(
            candidates, labels=labels, source=source, exclude=exclude
        )
        page, matched = bound_after_rank(
            annotated,
            key=lambda row: float(row.get("rank_more_dependent") or 10**9),
            limit=limit,
        )
        lineages.append({
            "lineage": lineage or path.stem,
            "association_status": "FOUND" if page else "NOT_RETAINED",
            "returned_count": len(page),
            "matched_row_count": matched,
            "common_essential_removed_count": meta["removed_count"],
            "rows": page,
        })
    return _evidence_response(
        "FOUND", mode="pan_cancer_dependency", ranking=ranking, gene=gene,
        reason="exact per-lineage states from completed tables" if gene else "bounded per-lineage rows from completed tables",
        lineages=lineages, lineage_count=len(lineages),
        exclude_common_essential_requested=exclude,
        common_essential_source=source,
        housekeeping_filter_applied=False,
        manifest=manifest, provenance=[str(root / "manifest.json"), *[str(path) for path in paths]],
    )


def _tf_activity_root(settings: Settings) -> Path:
    return (
        settings.knowledge_root
        / "analysis-modules"
        / "转录因子活性-CRISPR基因依赖相关性分析"
        / "results"
        / "tf_activity_dependency_26Q1_v2"
    )


def _tf_order_symbols(path: Path) -> set[str]:
    symbols: set[str] = set()
    if not path.is_file():
        return symbols
    for row in _iter_csv_records(path):
        symbol = row.get("TF") or row.get("symbol") or row.get("tf")
        if symbol:
            symbols.add(str(symbol).strip().upper())
    return symbols


def _tf_hits_path(root: Path) -> Path | None:
    for name in ("top_hits.csv.gz", "top_hits.csv"):
        path = root / name
        if path.is_file():
            return path
    return None


def _run_tf_dependency_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    """Read the installed TF-activity module in Python. Valid universe keys never 500."""
    source = str(query["source"]).strip().upper() if query.get("source") else None
    target = str(query["target"]).strip().upper() if query.get("target") else None
    view = "universe" if query.get("view") == "universe" else "ranking"
    limit = min(int(query.get("limit") or 20), 100)
    root = _tf_activity_root(settings)
    manifest, unavailable = _complete_module(root, mode="tf_dependency")
    if unavailable is not None:
        return unavailable
    order_path = root / "tf_order.csv"
    universe_rows = [
        {
            "symbol": str(row.get("TF") or row.get("symbol") or row.get("tf") or "").strip().upper(),
            "entity_class": "tf_activity",
        }
        for row in (_iter_csv_records(order_path) if order_path.is_file() else [])
        if str(row.get("TF") or row.get("symbol") or row.get("tf") or "").strip()
    ]
    universe = {row["symbol"] for row in universe_rows}
    if not universe:
        return _evidence_response(
            "COVERAGE_GAP",
            mode="tf_dependency",
            reason="the completed TF-activity module has no frozen TF universe table",
            source=source,
            target=target,
            view=view,
            entity_class="tf_activity",
            manifest=manifest,
            provenance=[str(root / "manifest.json")],
        )
    if view == "universe":
        page, matched = bound_after_rank(
            universe_rows, key=lambda row: row["symbol"], limit=limit
        )
        return _evidence_response(
            "FOUND",
            mode="tf_dependency",
            reason="bounded page of the frozen TF-activity universe; DoRothEA is not reconstructed",
            view="universe",
            entity_class="tf_activity",
            universe_size=len(universe),
            rows=page,
            returned_count=len(page),
            matched_row_count=matched,
            manifest=manifest,
            provenance=[str(root / "manifest.json"), str(order_path)],
        )
    hits_path = _tf_hits_path(root)
    if hits_path is None:
        return _evidence_response(
            "COVERAGE_GAP",
            mode="tf_dependency",
            reason="the completed module has no queryable TF-activity ranking table",
            source=source,
            target=target,
            entity_class="tf_activity",
            universe_size=len(universe),
            manifest=manifest,
            provenance=[str(root / "manifest.json"), str(order_path)],
        )
    if source is None:
        matched = list(_iter_csv_records(hits_path))
        page, matched_count = bound_after_rank(
            matched,
            key=lambda row: (
                int(row.get("rank") or 10**9),
                str(row.get("TF") or ""),
                str(row.get("target_gene") or ""),
            ),
            limit=limit,
        )
        return _evidence_response(
            "FOUND" if page else "NOT_RETAINED",
            mode="tf_dependency",
            reason="bounded bulk TF-activity ranking from the completed table",
            view="ranking",
            entity_class="tf_activity",
            universe_size=len(universe),
            rows=page,
            returned_count=len(page),
            matched_row_count=matched_count,
            manifest=manifest,
            provenance=[str(root / "manifest.json"), str(hits_path)],
        )
    if source not in universe:
        status = classify_exact_entity(observed=False)
        return _evidence_response(
            status,
            mode="tf_dependency",
            reason="requested symbol is absent from the frozen TF-activity universe",
            source=source,
            target=target,
            entity_class="tf_activity",
            universe_size=len(universe),
            rejection_reason=status,
            manifest=manifest,
            provenance=[str(order_path)],
        )
    if target:
        target_order = root / "target_gene_order.csv"
        if target_order.is_file():
            targets = {
                str(row.get("symbol") or row.get("target_gene") or "").strip().upper()
                for row in _iter_csv_records(target_order)
            }
            if target not in targets:
                return _evidence_response(
                    "NOT_TESTED",
                    mode="tf_dependency",
                    reason="target gene is absent from the Gene Effect target universe",
                    source=source,
                    target=target,
                    entity_class="tf_activity",
                    universe_size=len(universe),
                    rows=[],
                    returned_count=0,
                    manifest=manifest,
                    provenance=[str(order_path), str(target_order)],
                )
        matched = filter_before_limit(
            _iter_csv_records(hits_path),
            lambda row: str(row.get("TF") or "").upper() == source
            and str(row.get("target_gene") or "").upper() == target,
        )
        status = "FOUND" if matched else "NOT_RETAINED"
        return _evidence_response(
            status,
            mode="tf_dependency",
            reason=(
                "exact TF-activity pair from the completed ranking table"
                if matched
                else "TF is in the frozen universe; the pair was not retained in the ranking table"
            ),
            source=source,
            target=target,
            entity_class="tf_activity",
            universe_size=len(universe),
            rows=matched[:1],
            returned_count=len(matched[:1]),
            matched_row_count=len(matched),
            rejection_reason=None if status == "FOUND" else status,
            manifest=manifest,
            provenance=[str(root / "manifest.json"), str(hits_path)],
        )
    matched = filter_before_limit(
        _iter_csv_records(hits_path),
        lambda row: str(row.get("TF") or "").upper() == source,
    )
    page, matched_count = bound_after_rank(
        matched,
        key=lambda row: (
            str(row.get("direction") or ""),
            int(row.get("rank") or 10**9),
        ),
        limit=limit,
    )
    status = "FOUND" if page else "NOT_RETAINED"
    return _evidence_response(
        status,
        mode="tf_dependency",
        reason=(
            "bounded TF-activity ranking from the completed module"
            if page
            else "TF is in the frozen universe but the ranking table retained no rows"
        ),
        source=source,
        entity_class="tf_activity",
        universe_size=len(universe),
        rows=page,
        returned_count=len(page),
        matched_row_count=matched_count,
        rejection_reason=None if status == "FOUND" else status,
        manifest=manifest,
        provenance=[str(root / "manifest.json"), str(hits_path)],
    )


async def run_bounded_query(settings: Settings, query: dict[str, Any]) -> dict[str, Any]:
    if query["mode"] == "analysis_catalog":
        return await asyncio.to_thread(_run_analysis_catalog_query, settings, query)
    if query["mode"] == "mutation_anchor":
        return await asyncio.to_thread(_run_mutation_anchor_query, settings, query)
    if query["mode"] == "lineage_mutation_dependency":
        return await asyncio.to_thread(_run_lineage_mutation_dependency_query, settings, query)
    if query["mode"] == "lineage_catalog":
        return await asyncio.to_thread(_run_lineage_catalog_query, settings, query)
    if query["mode"] == "lineage_directions":
        return await asyncio.to_thread(_run_lineage_directions_query, settings, query)
    if query["mode"] == "lineage_dependency":
        return await asyncio.to_thread(_run_lineage_dependency_query, settings, query)
    if query["mode"] == "pan_cancer_dependency":
        return await asyncio.to_thread(_run_pan_cancer_dependency_query, settings, query)
    if query["mode"] == "core":
        result = await asyncio.to_thread(_run_core_query, settings, query["gene"])
        if not result["summary"]:
            return {
                "mode": "core",
                "status": "not_testable",
                "gene": query["gene"].strip().upper(),
                "reason": "gene is absent from the precomputed core index",
            }
        if len(json.dumps(result, ensure_ascii=False).encode("utf-8")) > MAX_RESPONSE_BYTES:
            raise HTTPException(status_code=413, detail="DepMap response exceeds 4 MiB")
        return result
    if query["mode"] == "tcga_expression_survival":
        return await asyncio.to_thread(_run_tcga_expression_survival_query, settings, query)
    if query["mode"] == "lineage_network":
        return await asyncio.to_thread(_run_lineage_network_query, settings, query)
    if query["mode"] == "lineage_cnv":
        return await asyncio.to_thread(_run_lineage_cnv_query, settings, query)
    if query["mode"] == "lineage_drug":
        return await asyncio.to_thread(_run_lineage_drug_query, settings, query)
    if query["mode"] == "enrichment":
        return await asyncio.to_thread(_run_enrichment_query, settings, query)
    if query["mode"] == "subtype":
        return await asyncio.to_thread(_run_subtype_query, settings, query)
    if query["mode"] == "coamplification":
        return await asyncio.to_thread(_run_coamplification_query, settings, query)
    if query["mode"] == "true_love":
        return await asyncio.to_thread(_run_true_love_query, settings, query)
    if query["mode"] == "biomarker_target":
        return await asyncio.to_thread(_run_biomarker_target_query, settings, query)
    if query["mode"] == "synthetic_lethal":
        if query.get("lineage"):
            redirected = {**query, "mode": "lineage_mutation_dependency"}
            return await asyncio.to_thread(
                _run_lineage_mutation_dependency_query, settings, redirected
            )
        return await asyncio.to_thread(_run_synthetic_lethal_query, settings, query)
    if query["mode"] == "three_d":
        return await asyncio.to_thread(_run_three_d_query, settings, query)
    if query["mode"] == "tf_dependency":
        return await asyncio.to_thread(_run_tf_dependency_query, settings, query)
    return await run_r_query(settings, query)


def create_app(settings: Settings | None = None, runner: Runner = run_bounded_query) -> FastAPI:
    @asynccontextmanager
    async def lifespan(api: FastAPI):
        if api.state.settings is None:
            api.state.settings = Settings.from_env()
        qa = verify_installation(api.state.settings)
        api.state.qa = qa
        api.state.semaphore = asyncio.Semaphore(api.state.settings.max_concurrency)
        yield

    api = FastAPI(
        title="Wisp Science DepMap Knowledge API",
        version="1.0.0",
        docs_url=None,
        redoc_url=None,
        openapi_url=None,
        lifespan=lifespan,
    )
    api.state.settings = settings
    api.state.runner = runner
    api.state.semaphore = None

    @api.exception_handler(RequestValidationError)
    async def provider_schema_validation(_request: Request, exc: RequestValidationError):
        messages = []
        for err in exc.errors():
            msg = err.get("msg") or "invalid argument"
            if msg.lower().startswith("value error, "):
                msg = msg[13:]
            messages.append(msg)
        return JSONResponse(
            status_code=422,
            content=schema_violation(reason="; ".join(messages) or "invalid provider arguments"),
        )

    @api.middleware("http")
    async def reject_large_requests(request: Request, call_next):
        content_length = request.headers.get("content-length")
        if content_length and int(content_length) > MAX_REQUEST_BYTES:
            return JSONResponse(status_code=413, content={"detail": "request too large"})
        return await call_next(request)

    def authorize(
        authorization: Annotated[str | None, Header()] = None,
    ) -> None:
        expected = f"Bearer {api.state.settings.api_token}"
        if authorization is None or not hmac.compare_digest(authorization, expected):
            raise HTTPException(
                status_code=status.HTTP_401_UNAUTHORIZED,
                detail="invalid DepMap API credential",
                headers={"WWW-Authenticate": "Bearer"},
            )

    @api.get("/api/v1/health", dependencies=[Depends(authorize)])
    async def health() -> dict[str, Any]:
        qa = verify_installation(api.state.settings)
        return {
            "schema_version": 1,
            "status": "ready",
            "release": qa["release"],
            "query_contract_version": QUERY_CONTRACT_VERSION,
            "coverage_manifest_version": 5,
            "qa_status": qa["qa_status"],
            "module_count": qa.get("module_count"),
            "query_modes": sorted(MODE_REQUIRED_FIELDS),
            "evidence_statuses": sorted(EVIDENCE_STATUSES),
        }

    @api.post("/api/v1/query", dependencies=[Depends(authorize)])
    async def query(payload: QueryRequest) -> dict[str, Any]:
        async with api.state.semaphore:
            return await api.state.runner(api.state.settings, payload.bounded_dict())

    return api


app = create_app()
