import subprocess,json,time,hashlib,sys
from pathlib import Path
root=Path('/data1/baiyifan/workplace/stock_market_game'); out=Path(__file__).resolve().parent; label=sys.argv[1]
evidence=out/label;evidence.mkdir(exist_ok=True); binary=Path('/tmp/tax-boundary-'+label)
source=out/'source-harness.rs'; module=out.parents[3]/'packages/engine/src/runtime/admin_tax_projection.rs'
args=['rustc','--test','--edition=2021','-C','codegen-units=8','-C','debuginfo=0','-C','opt-level=0','--extern','engine='+str(root/'.tmp/main-current-normal-rlib-2026-10-26/artifacts/libengine-e024975623ca4b70.rlib'),'-L','dependency='+str(root/'.tmp/main-current-normal-rlib-2026-10-26/target/debug/deps'),*sum([['--extern',name+'='+str(next((root/'.tmp/main-current-normal-rlib-2026-10-26/target/debug/deps').glob('lib'+name+'-*.rlib')))] for name in ['serde','serde_json','thiserror','ts_rs']],[]),str(source),'-o',str(binary)]
def run(cmd,name):
 start=time.monotonic();p=subprocess.run(['node',str(root/'scripts/run-with-deadline.mjs'),'10000','--']+cmd,capture_output=True,text=True);(evidence/(name+'.stdout')).write_text(p.stdout);(evidence/(name+'.stderr')).write_text(p.stderr);return {'argv':cmd,'exit':p.returncode,'wall_seconds':time.monotonic()-start}
sources=[source,*[(out.parents[3]/'packages/engine/src/company'/p) for p in ['cash_dividend_tax.rs','cash_dividend_tax/day_boundary.rs','cash_dividend_tax/day_boundary_tests.rs','cash_dividend_tax/entitlement_units.rs','cash_dividend_tax/entitlement_units/conditional_bonus.rs','share_registry.rs','share_registry/settlement_day.rs']]]; fingerprints={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}; [ (evidence/p.name).write_bytes(p.read_bytes()) for p in sources ]
receipt={'label':label,'source_before':fingerprints,'rlib_binding':str(root/'agents/branch-completion/main-current-normal-rlib-2026-10-26/artifact-binding.json'),'build':run(args,'build')}
if receipt['build']['exit']==0:
 receipt['binary_sha256']=hashlib.sha256(binary.read_bytes()).hexdigest();receipt['test']=run([str(binary),'day_boundary::tests','--test-threads=8','--nocapture'],'test')
receipt['source_after']={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}
(evidence/'receipt.json').write_text(json.dumps(receipt,ensure_ascii=False,indent=2)+'\n');print(json.dumps(receipt,ensure_ascii=False));
if receipt['build']['exit']: print((evidence/'build.stderr').read_text())
else: print((evidence/'test.stdout').read_text());print((evidence/'test.stderr').read_text())

sys.exit(receipt['build']['exit'] or receipt.get('test',{}).get('exit',0))
