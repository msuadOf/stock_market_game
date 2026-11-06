from pathlib import Path
import concurrent.futures,hashlib,json,os,runpy,stat,subprocess,sys
here=Path(__file__).resolve().parent;work=Path('/data1/<USER>/workplace/stock_market_game/.worktree/d3-p1-integration-2026-10-28');root=Path('/data1/<USER>/workplace/stock_market_game');target=Path('/tmp/stock-market-game-validation-2026-10-27-1311109/target')
if len(sys.argv)!=5:raise RuntimeError('必须具名传root实际前槽front、SHA、session、WAIT chunk，不自动猜最新')
front=Path(sys.argv[1]);front_sha=sys.argv[2];session=int(sys.argv[3]);chunk=sys.argv[4]
def sha(p):
 with Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def put(name,x):(here/name).write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def met(p):
 p=Path(p);s=p.lstat()
 if not stat.S_ISREG(s.st_mode):raise RuntimeError('现件必须regular')
 return {'path':str(p),'dev':s.st_dev,'inode':s.st_ino,'bytes':s.st_size,'mtimeNs':s.st_mtime_ns}
def git(*args):return subprocess.check_output(['git','-C',str(work),*args],text=True).strip()
if not front.is_absolute() or sha(front)!=front_sha:raise RuntimeError('root前槽front绝对路径/小SHA不等')
f=json.loads(front.read_text())
if (f['actualSessionId'],f['actualWaitChunk'])!=(session,chunk) or f['actualWaitExit'] not in [0,1]:raise RuntimeError('真实WAIT错位')
if any(f[k] for k in ['remainingOwnedIdentities','remainingExactArgvPids','remainingCargoRustc','scratchExists']):raise RuntimeError('前槽未清场')
plan=json.loads((here/'plan-template.json').read_text())
if (git('rev-parse','HEAD'),git('rev-parse','HEAD^{tree}'),git('status','--porcelain=v1'))!=(plan['candidate'],plan['candidateTree'],plan['candidateStatus']):raise RuntimeError('冻结Git来源漂移')
for row in plan['runnerSources']:
 if sha(row['path'])!=row['sha256']:raise RuntimeError('runner来源漂移')
def verify(row):
 if sha(work/row['path'])!=row['sha256']:raise RuntimeError('冻结源码漂移：'+row['path'])
with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:list(pool.map(verify,plan['productSources']+plan['webProductSources']))
priors=[]
quota=runpy.run_path(str(work/'agents/branch-completion/wasm-w1/combined/fresh_binding.py'))['tmp_user_quota']();sv=os.statvfs(target)
if sv.f_bavail*sv.f_frsize<20*1024**3 or quota['availableBytes']<5*1024**3:raise RuntimeError('RAM20GiB/本用户真实quota5GiB不足')
for p in Path('/proc').iterdir():
 if p.name.isdecimal():
  try:
   if (p/'comm').read_text().strip() in ['cargo','rustc','wasm-pack']:raise RuntimeError('有其他实际build，不接唯一窗口')
  except (FileNotFoundError,ProcessLookupError):pass
if (target.parent/'engine-r4-payment-red-2026-10-29-01').exists() or (here/'batch-receipt.json').exists():raise RuntimeError('已有本批原件，禁止重复launch')
plan['mainHelperHead']=subprocess.check_output(['git','-C',plan['mainHelperRepository'],'rev-parse','HEAD'],text=True).strip()
put('plan-template.json',plan);put('runner-freeze.json',{'files':plan['runnerSources']+[{'path':str(here/'plan-template.json'),'sha256':sha(here/'plan-template.json')}]})
put('slot-handoff.json',{'pendingSlotHandoff':False,'rootAuthorized':True,'authority':'root正式唯一后继窗口；全部准入成功才可独立launch','actualWaitReceipt':str(front),'actualWaitReceiptSha256':front_sha,'artifactArchiveMaps':[],'historicalScope':'未读核继承链；只是前槽as-of小摘要，本批无旧制品直接输入','artifacts':priors,'expectedPlanSha256':sha(here/'plan-template.json'),'expectedSourceFreezeSha256':sha(here/'source-freeze.json')})
put('preflight.json',{'status':'READY','quota':quota,'front':str(front),'frontSha256':front_sha,'actualSession':session,'actualWaitChunk':chunk,'planSha256':sha(here/'plan-template.json'),'sourceCount':len(plan['productSources']),'webCount':len(plan['webProductSources'])})
print(json.dumps({'status':'READY','quota':quota['availableBytes'],'plan':sha(here/'plan-template.json')}))
