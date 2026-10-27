# 普通税日源码轻量验证；借已绑定 BC engine，仅验证此处编入的真实来源叶。
import hashlib,json,shutil,subprocess,sys,time
from pathlib import Path
root=Path.cwd(); out=Path(__file__).resolve().parent; label=sys.argv[1]
evidence=out/label; evidence.mkdir(exist_ok=False)
work=Path('/tmp')/('tax-source-'+label); work.mkdir(exist_ok=False)
binding=Path('/data1/baiyifan/workplace/stock_market_game/.worktree/company-catalog-bc/agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/artifact-binding.json')
normal=json.loads(binding.read_text())['normalRlibs']; deps=Path(normal['engine']['path']).parent
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
for item in normal.values():
 if sha(Path(item['path']))!=item['sha256']: raise RuntimeError('已有 rlib 指纹不符：'+item['path'])
company=root/'packages/engine/src/company'
paths=[company/'share_registry.rs',company/'cash_dividend_tax.rs',*sorted((company/'share_registry').rglob('*.rs')),*sorted((company/'cash_dividend_tax').rglob('*.rs'))]
for path in paths:
 target=work/'company'/path.relative_to(company); target.parent.mkdir(parents=True,exist_ok=True); shutil.copyfile(path,target)
(work/'company.rs').write_text('pub use engine::company::*;\npub mod share_registry;\npub mod cash_dividend_tax;\n')
harness=(out/'source-harness.rs').read_text(); harness=harness[:harness.index('mod company {')]
harness=harness.replace('pub use engine::{account,calendar,money,runtime,Money,AccountId,Side};','pub use engine::{account,money,runtime,Money,AccountId,Side};\nmod calendar { pub use engine::calendar::*; #[path="'+str(root/'packages/engine/src/calendar/date.rs')+'"] mod local_date; pub(crate) use local_date::days_in_month; }')
personal=root/'packages/engine/src/session/personal_trade_history.rs'
personal_prefix=personal.read_text().split('impl super::GameSession {')[0]
(work/'personal.rs').write_text(personal_prefix)
harness+='pub use engine::{config,fees,stock};\nmod u64_decimal { pub use crate::orderbook::canonical_u64_decimal::{serialize,deserialize}; }\nmod session { #[path="'+str(work/'personal.rs')+'"] pub(crate) mod personal_trade_history; }\n'
paths.append(personal)
(work/'main.rs').write_text(harness+'mod company;\n')
fingerprints={str(p):sha(p) for p in paths}; (evidence/'source-before.json').write_text(json.dumps(fingerprints,indent=2)+'\n')
# 依赖由该 engine 的实际 fingerprint 引用选择，禁止任取同名 rlib。
engine_fp=json.loads((deps.parent/'.fingerprint'/('engine-'+Path(normal['engine']['path']).stem.split('-')[-1])/'lib-engine.json').read_text())
externs={'engine':normal['engine']['path'],'serde_json':normal['serde_json']['path']}
for _,name,_,fp_hash in engine_fp['deps']:
 if name not in ['serde','thiserror','ts_rs','num_bigint']: continue
 candidates=[]
 for lib in deps.glob('lib'+name+'-*.rlib'):
  fp=deps.parent/'.fingerprint'/(name.replace('_','-')+'-'+lib.stem.split('-')[-1])/('lib-'+name)
  if fp.exists() and int.from_bytes(bytes.fromhex(fp.read_text().strip()),'little')==fp_hash: candidates.append(lib)
 if len(candidates)!=1: raise RuntimeError('依赖 fingerprint 不唯一：'+name+' '+str(candidates))
 externs[name]=str(candidates[0])
args=['rustc','--test','--edition=2021','-C','codegen-units=8','-C','debuginfo=0','-L','dependency='+str(deps)]
for name,path in externs.items(): args+=['--extern',name+'='+path]
args += [str(work/'main.rs'),'-o',str(work/'tests')]
def run(cmd,name):
 start=time.monotonic(); p=subprocess.run(['node',str(root/'scripts/run-with-deadline.mjs'),'10000','--']+cmd,capture_output=True,text=True)
 (evidence/(name+'.stdout')).write_text(p.stdout); (evidence/(name+'.stderr')).write_text(p.stderr)
 return {'argv':cmd,'exit':p.returncode,'wall_seconds':time.monotonic()-start}
receipt={'label':label,'binding':str(binding),'borrowed_rlibs':{name:{'path':path,'sha256':sha(Path(path))} for name,path in externs.items()},'source_before':fingerprints,'build':run(args,'build')}
if receipt['build']['exit']==0:
 receipt['binary_sha256']=sha(work/'tests'); receipt['test']=run([str(work/'tests'),'day_boundary::tests','--test-threads=8','--nocapture'],'test')
receipt['source_after']={str(p):sha(p) for p in paths}; (evidence/'receipt.json').write_text(json.dumps(receipt,ensure_ascii=False,indent=2)+'\n')
print(json.dumps(receipt,ensure_ascii=False)); print((evidence/('build.stderr' if receipt['build']['exit'] else 'test.stdout')).read_text())
sys.exit(receipt['build']['exit'] or receipt.get('test',{}).get('exit',0))
