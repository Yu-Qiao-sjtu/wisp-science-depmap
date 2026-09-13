#!/usr/bin/env python3
import argparse,json
from pathlib import Path
def main():
 p=argparse.ArgumentParser();p.add_argument('--knowledge-root',required=True);a=p.parse_args();root=Path(a.knowledge_root)/'depmap-26q1-3d';fail=[]
 for module in ('codependency','true_love_gene'):
  path=root/module/'manifest.json'
  try:d=json.loads(path.read_text())
  except Exception:d={}
  if d.get('status')!='complete':fail.append(f'{module}: incomplete')
 for cohort in ('all_3d_lineage_adjusted','organoid_3do_lineage_adjusted'):
  path=root/'codependency'/cohort/'manifest.json'
  try:d=json.loads(path.read_text())
  except Exception:d={}
  if d.get('lineage_adjusted') is not False:fail.append(f'{cohort}: expected raw TM00 Pearson')
 result={'schema_version':1,'family':'tm00_script17_3d','status':'complete' if not fail else 'failed','qa_status':'PASS' if not fail else 'FAIL','model_scope':'QC-passing 3D only','method':'raw Pearson Gene Effect correlation followed by strict mutual negative rank-1; no bootstrap; no lineage residualization','failures':fail}
 (root/'tm00_script17_catalog.json').write_text(json.dumps(result,indent=2));print(json.dumps(result));return 0 if not fail else 1
if __name__=='__main__':raise SystemExit(main())
