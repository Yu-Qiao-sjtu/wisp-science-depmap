#!/usr/bin/env python3
from __future__ import annotations
import argparse, csv, json
from datetime import datetime, timezone
from pathlib import Path

EXPECTED = {
    "audit": ["catalog.json", "file_catalog.csv"],
    "dependency_profiles": ["manifest.json", "group_catalog.csv"],
    "differential_dependency": ["manifest.json", "contrast_catalog.csv"],
    "codependency": ["manifest.json", "cohort_catalog.csv"],
    "true_love_gene": ["manifest.json", "cohort_catalog.csv"],
    "omics_dependency": ["manifest.json", "modality_catalog.csv"],
    "lineage_dependency_enrichment": ["manifest.json", "group_catalog.csv"],
}

def load(path: Path): return json.loads(path.read_text(encoding="utf-8-sig"))

def main():
    p=argparse.ArgumentParser();p.add_argument("--knowledge-root",required=True);a=p.parse_args()
    root=Path(a.knowledge_root).resolve()/"depmap-26q1-3d";fail=[];mods=[]
    for name,files in EXPECTED.items():
        mr=root/name
        for f in files:
            if not (mr/f).is_file() or (mr/f).stat().st_size==0: fail.append(f"{name}: missing {f}")
        mp=mr/("catalog.json" if name=="audit" else "manifest.json")
        manifest=load(mp) if mp.is_file() else {}
        status=manifest.get("status")
        if status!="complete": fail.append(f"{name}: status={status}")
        if manifest.get("qa_status") not in (None,"PASS"): fail.append(f"{name}: qa failed")
        mods.append({"module":name,"status":status,"path":str(mr)})
    qa={"schema_version":1,"release":"NextGen Model Manuscript 2026","generated_at":datetime.now(timezone.utc).isoformat(),"status":"complete" if not fail else "failed","qa_status":"PASS" if not fail else "FAIL","module_count":len(mods),"modules":mods,"failures":fail,"scope":"standalone NextGen 3D knowledge layer; 2D comparison retained only as validation"}
    (root/"catalog.json").write_text(json.dumps(qa,indent=2),encoding="utf-8")
    with (root/"catalog.csv").open("w",newline="",encoding="utf-8") as h:
        w=csv.DictWriter(h,fieldnames=["module","status","path"]);w.writeheader();w.writerows(mods)
    print(json.dumps({"qa_status":qa["qa_status"],"failures":fail}));return 0 if not fail else 1
if __name__=="__main__": raise SystemExit(main())
