#!/usr/bin/env python3
import argparse,json
from datetime import datetime,timezone
from pathlib import Path

def main():
 p=argparse.ArgumentParser();p.add_argument('--knowledge-root',required=True);a=p.parse_args();root=Path(a.knowledge_root)/'depmap-26q1-3d'
 expected={'extended_analysis_audit':['manifest.json','lineage_feasibility.csv','amplification_prevalence.csv.gz'],'true_love_stability':['manifest.json','cohort_catalog.csv'],'coamplification_dependency':['manifest.json','eligible_pairs.csv','significant_within_pair.csv.gz','significant_global_fdr.csv.gz'],'integrated_validation':['manifest.json','true_love_2d_3d_validation.csv.gz','stable_true_love_geneset_enrichment.csv']}
 failures=[];modules=[]
 for name,files in expected.items():
  d=root/name;manifest={}
  for f in files:
   pth=d/f
   if not pth.is_file() or pth.stat().st_size==0:failures.append(f'{name}: missing {f}')
  try:manifest=json.loads((d/'manifest.json').read_text())
  except Exception:pass
  if manifest.get('status')!='complete':failures.append(f'{name}: status={manifest.get("status")}')
  if manifest.get('qa_status')!='PASS':failures.append(f'{name}: qa_status={manifest.get("qa_status")}')
  modules.append({'module':name,'status':manifest.get('status')})
 result={'schema_version':1,'generated_at':datetime.now(timezone.utc).isoformat(),'status':'complete' if not failures else 'failed','qa_status':'PASS' if not failures else 'FAIL','modules':modules,'failures':failures,'scientific_boundaries':['No de novo lineage-specific co-dependency discovery below 20 models','WGCNA and predictive models excluded by project scope','Associations are hypothesis-generating, not causal']}
 (root/'extended_catalog.json').write_text(json.dumps(result,indent=2));print(json.dumps(result));return 0 if not failures else 1
if __name__=='__main__':raise SystemExit(main())
