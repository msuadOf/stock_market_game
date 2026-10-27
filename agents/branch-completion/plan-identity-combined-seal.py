# 最终机器 manifest：历史未捕获的时序字段明确留空，不追写成原始收据。
import concurrent.futures,hashlib,json,pathlib,re,subprocess
root=pathlib.Path('/data1/baiyifan/workplace/stock_market_game');repo=root/'.worktree/plan-identity-combined';out=root/'.tmp/plan-identity-combined'
def git(*args):return subprocess.check_output(['git',*args],cwd=repo,text=True).strip()
def seal(path):
 path=pathlib.Path(path)
 with path.open('rb') as file:digest=hashlib.file_digest(file,'sha256').hexdigest()
 return {'path':str(path),'bytes':path.stat().st_size,'sha256':digest}
before=json.loads((out/'source-before-build.json').read_text());after=json.loads((out/'source-final-after-tests.json').read_text());a={f['path']:f for f in before['files']};b={f['path']:f for f in after['files']};changed=[p for p in sorted(a.keys()|b.keys()) if a.get(p)!=b.get(p)]
assert len(changed)==4 and all(p.startswith('apps/web/') for p in changed)
assert git('rev-parse','HEAD')=='c8e9a4f51dfb9021cf95b6da8f7f52cfbcf20957'
patch=out/'combined-final-complete.patch'
with patch.open('wb') as f:subprocess.run(['git','diff','c0138972c687a197164fca81d56a9bdc116b4517','HEAD','--binary'],cwd=repo,stdout=f,check=True)
receipts=[json.loads(p.read_text()) for p in sorted(out.glob('final-*-receipt.json'))]
for receipt in receipts:
 assert all(r['exit']==0 for r in receipt['results'])
 for r in receipt['results']:
  text=pathlib.Path(r['stdout']).read_text();r['summary']=re.findall(r'test result:[^\n]*|[ℹ#] (?:tests|pass|fail|cancelled|skipped) \d+',text)
  if any(x.startswith('test result:') for x in r['summary']):assert any('ok.' in x and 'ok. 0 passed' not in x for x in r['summary'])
chain=json.loads((out/'rust-related-inventory.json').read_text())['chain'];seen=[]
for r in next(x for x in receipts if x['mode']=='rust')['results']:
 if r['label'].startswith('chain-'):seen+=re.findall(r'^test (\S+) \.\.\. ok$',pathlib.Path(r['stdout']).read_text(),re.M)
assert len(chain)==71 and sorted(chain)==sorted(seen) and len(set(seen))==71
base=next(x for x in receipts if x['mode']=='base');unit_names=[]
for receipt in receipts:
 for r in receipt['results']:
  if 'engine-3eb2aa70a85e274e' in ' '.join(r['argv']):unit_names+=re.findall(r'^test (\S+) \.\.\. ok$',pathlib.Path(r['stdout']).read_text(),re.M)
paths=[p for p in out.iterdir() if p.is_file() and p.name!='final-manifest.json']
paths += [root/'agents/branch-completion'/n for n in ['plan-identity-combined-build.py','plan-identity-combined-final-checks.py','plan-identity-combined-freeze.mjs','plan-identity-combined-seal.py','plan-identity-combined-producer.rs','plan-identity-combined-producer-v2.rs','plan-identity-combined-producer-v3.rs','plan-identity-combined-web-roundtrip.mjs']]
paths += [pathlib.Path(a['path']) for a in json.loads((out/'build-artifact-seals.json').read_text())['artifacts'] if pathlib.Path(a['path']).is_file()]
with concurrent.futures.ThreadPoolExecutor(max_workers=5) as pool:seals=list(pool.map(seal,dict.fromkeys(paths)))
for item in json.loads((out/'build-artifact-seals.json').read_text())['artifacts']:
 assert next(s for s in seals if s['path']==item['path'])['sha256']==item['sha256']
assert (out/'web-full-save-v3.json').read_bytes()==(out/'web-full-save-v3-receipted.json').read_bytes()
manifest={'status':'writer 验收已通过；等待未实施 reviewer 最终裁决','production_head':git('rev-parse','HEAD'),'root_tree':git('rev-parse','HEAD^{tree}'),'web_tree':git('rev-parse','HEAD:apps/web'),'rust_build_head':'1579b8b320a93188514672196d99e94f33de0f3a','rust_build_source_digest':before['digest'],'final_source_digest':after['digest'],'rust_build_input_files':len(before['files']),'final_input_files':len(after['files']),'only_source_delta_after_rust_build':changed,'rust_inputs_unchanged':True,'source_before_after_build_equal':before==json.loads((out/'source-after-rust-build.json').read_text()),'working_tree':git('status','--short'),'web_case_count':222,'web_file_count':36,'chain_exactly_once':{'cases':71,'slices':9,'verified':True},'unit_case_invocations_with_repeats':len(unit_names),'unit_distinct_cases':len(set(unit_names)),'receipts':receipts,'historical_compile_metadata':{'v2_v3':'原stdout/stderr与源码/binary SHA完整；当时 standalone rustc 无集中 argv/时序收据，不能伪称历史原件','known_invocation_template':['node','scripts/run-long-validation.mjs','300000','--','rustc','--edition=2021','-C','debuginfo=0','-C','codegen-units=16','<v2或v3源码>','-L','dependency=<组合target/debug/deps>','--extern','engine=<组合libengine-b7714b84cb9f18d0.rlib>','--extern','serde_json=<libserde_json-910c725843be2f8c.rlib>','-o','<v2或v3binary>'],'cwd':str(repo),'wall_ms':None,'source':'先前实施记录及本次组合产物链接关系；非新捕获原件'},'limitations':['未运行full/all','未证明Simple/月报真实生产旅程；Monthly仅parser回归','未实施#122；等待独立I-D裁决','旧producer v1/v2断言失败日志保留；v3 inst8真实自然生产成功','不同真实并发调度的producer重放不宣称JSON字节确定性'],'artifact_seals':seals}
(out/'final-manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({k:manifest[k] for k in ['production_head','root_tree','web_tree','final_source_digest','unit_case_invocations_with_repeats','unit_distinct_cases','working_tree']},ensure_ascii=False))
