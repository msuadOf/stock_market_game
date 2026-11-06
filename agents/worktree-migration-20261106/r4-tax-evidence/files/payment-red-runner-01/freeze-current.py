from pathlib import Path
import concurrent.futures,hashlib,json,subprocess,ast
root=Path('/data1/<USER>/workplace/stock_market_game');work=Path.cwd();here=Path(__file__).resolve().parent
old=Path('/data1/<USER>/workplace/stock_market_game/.worktree/d3-p1-integration-2026-10-28/agents/branch-completion/joint-green50-r2-2026-10-29');helper=root/'.worktree/main-merge-2026-10-25'
if work != Path('/data1/<USER>/workplace/stock_market_game/.worktree/d3-p1-integration-2026-10-28'):
 raise RuntimeError('freeze-current.py仅允许在固定integration工作树cwd执行')
receiver=json.loads((root/'.worktree/r4-tax-production-2026-10-29/agents/branch-completion/r4-tax-production-2026-10-29/payment-three-red-source-01-fingerprints.json').read_text())
def sha(p):
 with Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
for row in receiver['paths']:
 if sha(work/row['path']) != row['sha256']:
  raise RuntimeError('接收树付款tests-only source SHA未达到付款freeze：'+row['path'])
def put(name,x):(here/name).write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def git(*args):return subprocess.check_output(['git','-C',str(work),*args],text=True).strip()
files=git('ls-files', '--cached', '--others', '--exclude-standard').splitlines()
rust_names=sorted(p for p in files if p.endswith('.rs') or Path(p).name in ['Cargo.toml','Cargo.lock','rust-toolchain.toml'] or p.endswith('.cargo/config.toml') or p=='LICENSE')
old_plan=json.loads((old/'plan-template.json').read_text())
web_names=sorted(set(r['path'] for r in old_plan['webProductSources']) | {p for p in files if p.startswith('apps/web/') and not p.endswith('/Cargo.toml')})
def source(p):return {'path':p,'bytes':(work/p).stat().st_size,'sha256':sha(work/p)}
with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
 rust=list(pool.map(source,rust_names));web=list(pool.map(source,web_names))
put('source-freeze.json',rust);put('source-freeze-web.json',web)
plan={**json.loads((here/'plan-template-seed.json').read_text()),'candidate':git('rev-parse','HEAD'),'candidateTree':git('rev-parse','HEAD^{tree}'),'candidateStatus':git('status','--porcelain=v1'),'mainHelperHead':subprocess.check_output(['git','-C',str(helper),'rev-parse','HEAD'],text=True).strip(),'productSources':rust,'webProductSources':web,'launchReadiness':'SOURCE_READY_WAIT_LATEST_FRONT_AND_ROOT_SLOT'}
cmd=json.loads((here/'cases-plan.json').read_text())['batches'][0]['command'];plan['command']=cmd;plan['cargoCommand']=cmd
plan['environment']['TMPDIR']='/tmp/stock-market-game-validation-2026-10-27-1311109/engine-r4-payment-red-2026-10-29-01/scratch'
put('snapshot-plan.json',plan)
plan['expectedNativeCargoInputs']=json.loads(subprocess.check_output([plan['nodePath'],str(here/'build-verified.mjs'),str(here/'snapshot-plan.json'),'native','--snapshot-only'],text=True,cwd=work))
control=[here/n for n in ['run-ram-batch.py','launch-batch.py','absolute-supervisor.mjs','cleanup-supervisor.mjs','subreaper-owner.py','owned-native-source.py','build-verified.mjs','terminate-owned.mjs','cases-plan.json','expected-red-contracts.json','source-freeze.json','source-freeze-web.json','bind-preflight.py','publish-actual-front.py']]
control += [Path(plan['nativeGuard']),helper/'scripts/run-with-deadline.mjs',helper/'scripts/run-long-validation.mjs',root/'agents/branch-completion/funds-production-resume-2026-10-27/p1-formal-red-2026-10-28/collect-child-status.py',work/'agents/branch-completion/wasm-w1/combined/fresh_binding.py',work/'agents/branch-completion/wasm-w1/combined/owner.py',work/'agents/branch-completion/wasm-w1/combined/cargo-inputs.mjs']
plan['runnerSources']=[{'path':str(p),'sha256':sha(p)} for p in control]
put('plan-template.json',plan);put('runner-freeze.json',{'files':plan['runnerSources']+[{'path':str(here/'plan-template.json'),'sha256':sha(here/'plan-template.json')}]})
for p in here.glob('*.py'):ast.parse(p.read_text(),filename=str(p))
print(json.dumps({'candidate':plan['candidate'],'rust':len(rust),'web':len(web),'helper':plan['mainHelperHead'],'planSha256':sha(here/'plan-template.json')}))
