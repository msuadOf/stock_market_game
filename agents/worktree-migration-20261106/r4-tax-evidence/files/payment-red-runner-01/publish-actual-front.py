from pathlib import Path
import hashlib,json,runpy,sys
here=Path(__file__).resolve().parent;work=Path('/data1/<USER>/workplace/stock_market_game/.worktree/d3-p1-integration-2026-10-28')
if len(sys.argv)!=4:raise RuntimeError('必须以真实工具WAIT传session/exit/chunk，不预写结果')
session,exit_code,chunk=int(sys.argv[1]),int(sys.argv[2]),sys.argv[3]
b=json.loads((here/'batch-receipt.json').read_text());s=json.loads((here/'supervisor-receipt.json').read_text())
def sha(p):
 with Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def met(p):
 p=Path(p);x=p.lstat();return {'path':str(p),'dev':x.st_dev,'inode':x.st_ino,'bytes':x.st_size,'mtimeNs':x.st_mtime_ns}
roles={}
for a in b['artifacts']:
 role={'engine':'feature-unit','company_r4_tax_production':'feature-tax-integration','runtime_diagnostics_d1':'feature-d1-integration'}[a['target']]
 roles[role]={**met(a['path']),'role':role,'sha256':a['sha256'],'source':a['cargoArtifact']['target']['src_path'],'features':a['features'],'fresh':a['fresh'],'profile':a['cargoArtifact']['profile'],'executionBinding':a['executionBinding'],'scope':'仅本次同源实际产物，正式预算内完整SHA；本批没有跨批大seal复用需求'}
for a in b.get('normalArtifactFiles',[]):
 role='feature-normal-'+Path(a['path']).suffix[1:]
 roles[role]={**met(a['path']),'role':role,'sha256':a['sha256'],'source':a['cargoArtifact']['target']['src_path'],'features':a['cargoArtifact']['features'],'fresh':a['cargoArtifact']['fresh'],'profile':a['cargoArtifact']['profile'],'scope':'仅本次同源normal实际输出；按root政策不复制无消费者旧件或seal'}
owners=[];cargo=[];argv_pids=[]
exact_argv=[t['childStatus']['argv'] for t in b['tests'] if t.get('childStatus')]
for p in Path('/proc').iterdir():
 if not p.name.isdecimal():continue
 try:
  raw=(p/'stat').read_text();fields=raw[raw.rfind(')')+2:].split();comm=(p/'comm').read_text().strip();argv=(p/'cmdline').read_bytes().decode().split('\0')[:-1]
  if fields[0] not in ['Z','X']:
   if any(int(old['pid'])==int(p.name) and str(old['birth'])==fields[19] for old in b['observedOwnedProcesses']):owners.append({'pid':int(p.name),'birth':fields[19],'comm':comm})
   if comm in ['cargo','rustc','wasm-pack']:cargo.append({'pid':int(p.name),'comm':comm,'argv':argv})
   if argv in exact_argv:argv_pids.append({'pid':int(p.name),'argv':argv})
 except (FileNotFoundError,ProcessLookupError):pass
counts={'selected':3,'executed':len(b['tests']),'rawBusinessRed':sum(t['rawBusinessCaseRed'] for t in b['tests']),'expectedRed':sum(t['expectedTarget'].get('expected') == 'red' and t['businessCaseRed'] for t in b['tests']),'green':sum(t['exactPassed'] for t in b['tests']),'expectedMatched':sum(t['expectedMatched'] for t in b['tests'])}
x={'classification':b['status'],'actualSessionId':session,'actualWaitExit':exit_code,'actualWaitChunk':chunk,'batchReceipt':str(here/'batch-receipt.json'),'batchReceiptSha256':sha(here/'batch-receipt.json'),'supervisorReceipt':str(here/'supervisor-receipt.json'),'supervisorReceiptSha256':sha(here/'supervisor-receipt.json'),'wallSeconds':b['wallSecondsIncludingCleanup'],'outerWallSeconds':s['wallMs']/1000,'remainingOwnedIdentities':owners,'remainingExactArgvPids':argv_pids,'remainingCargoRustc':cargo,'scratchExists':Path(s['scratch']).exists(),'ownedSourceMtimeRestored':s.get('ownedSourceTimesRestored',False),'current':list(roles.values()),'currentArtifacts':list(roles.values()),'roleArtifacts':roles,'counts':counts,'historicalScope':'没有继承或提升任何历史角色；current只含本批实际接受artifact，未产出则为空','notRehashedAfterWait':True,'quotaAfter':runpy.run_path(str(work/'agents/branch-completion/wasm-w1/combined/fresh_binding.py'))['tmp_user_quota']()}
p=here/'actual-wait-cleanup-front.json';p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'path':str(p),'sha256':sha(p),'counts':counts,'owned':owners,'cargo':cargo,'scratchExists':x['scratchExists']}))
