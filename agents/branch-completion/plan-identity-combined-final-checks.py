# I-D 最终短门：记录真实 argv、退出码、原件与源码绑定；不构建 Rust。
import concurrent.futures, hashlib, json, os, pathlib, re, shutil, subprocess, sys, time
root=pathlib.Path('/data1/baiyifan/workplace/stock_market_game');repo=root/'.worktree/plan-identity-combined';out=root/'.tmp/plan-identity-combined';mode=sys.argv[1]
def run(job):
 label,argv,cwd,limit=job
 command=['node',str(repo/'scripts'/('run-long-validation.mjs' if limit==300000 else 'run-with-deadline.mjs')),str(limit),'--',*map(str,argv)]
 t=time.monotonic()
 with (out/(label+'.stdout')).open('wb') as stdout,(out/(label+'.stderr')).open('wb') as stderr:r=subprocess.run(command,cwd=cwd,stdout=stdout,stderr=stderr)
 text=(out/(label+'.stdout')).read_text();summary=re.findall(r'test result:[^\n]*|# (?:tests|pass|fail|cancelled|skipped) \d+',text)
 return dict(label=label,argv=command,cwd=str(cwd),exit=r.returncode,wall_ms=round((time.monotonic()-t)*1000),deadline_ms=limit,summary=summary,stdout=str(out/(label+'.stdout')),stderr=str(out/(label+'.stderr')))
if mode=='types-child':
 jobs=[('tsc-'+name,['node',repo/'apps/web/node_modules/typescript/bin/tsc','-p',repo/('apps/web/tsconfig.'+config+'.json'),'--incremental','false'],repo,300000) for name,config in [('app','app'),('node','node'),('DEV','inspector')]]
 # 外层还有一个共享300000ms进程树deadline，三项目同时执行。
elif mode=='web':
 original=json.loads((repo/'agents/branch-completion/plan-identity-web-final-validation.json.log').read_text())
 files=original['普通测试']['command'][8:]
 assert len(files)==35,files
 monthly='src/save/schema/company/monthly-origin.test.ts'
 assert monthly not in files;files.append(monthly)
 (out/'web-files.json').write_text(json.dumps(files,indent=2)+'\n')
 for file in ['Cargo.lock','apps/desktop/src-tauri/Cargo.toml','apps/desktop/src-tauri/capabilities/default.json','apps/desktop/src-tauri/tauri.conf.json']:
  assert (repo/file).read_bytes()==(root/file).read_bytes(),file
 cache='apps/desktop/src-tauri/gen/schemas/acl-manifests.json';(repo/cache).parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(root/cache,repo/cache)
 (out/'web-cache.json').write_text(json.dumps({'source':str(root/cache),'target':str(repo/cache),'sha256':hashlib.sha256((repo/cache).read_bytes()).hexdigest(),'note':'仅复用同依赖 ACL cache，不声称 Tauri 构建'},ensure_ascii=False,indent=2)+'\n')
 jobs=[('web-short',['node','--test','--test-timeout=10000','--test-concurrency=8',*files],repo/'apps/web',10000),('web-lint',['node','node_modules/oxlint/bin/oxlint','--deny-warnings','src/save','src/host/npc-decision-trace.ts','src/host/npc-decision-trace.test.ts','src/host/public-report-normalize.ts','src/utils/plan-identity.ts','src/dev/NpcDecisionInspector.tsx','src/dev/npc-decision-inspector.test.ts'],repo/'apps/web',10000)]
elif mode=='extra':
 binary=out/'target/debug/deps/engine-3eb2aa70a85e274e'
 jobs=[('roster-consumers',[binary,'runtime::world::group_roster_consumer_tests::','--test-threads=8'],repo,10000),('producer-v3-replay',[out/'combined-producer-v3','produce',out/'rust-full-save-v3-replay.json'],repo,10000)]
elif mode=='journey':
 helper=root/'agents/branch-completion/plan-identity-combined-web-roundtrip.mjs'
 jobs=[('web-roundtrip-receipted',['node','--experimental-strip-types',helper,repo,out/'rust-full-save-v3.json',out/'web-full-save-v3-receipted.json'],repo,10000)]
elif mode=='restore':
 jobs=[('rust-roundtrip-receipted',[out/'combined-producer-v3','restore',out/'web-full-save-v3-receipted.json'],repo,10000)]
elif mode=='base':
 target=out/'target/debug/deps'
 specs=[('unit-identity-final','engine-3eb2aa70a85e274e',['plan_identity']),('purse-final','engine-3eb2aa70a85e274e',['runtime::group::pending_cash_tests::']),('core-identity-final','plan_identity-4c6cc6b4c30fbbdd',['--skip','export_bindings_plan_identity']),('behavior-final','plan_identity_behavior-4ba0cb78c616347d',[]),('allocation-final','plan_allocation-66dad3f56f8ab836',[]),('save-contract-final','save_contract-eca1eeae557b8b6d',['restore_guards'])]
 jobs=[(label,[target/binary,*args,'--test-threads=8'],repo,10000) for label,binary,args in specs]
elif mode=='rust':
 binary=out/'target/debug/deps/engine-3eb2aa70a85e274e'
 names=[line[:-6] for line in (out/'unit-list.txt').read_text().splitlines() if line.endswith(': test')]
 jobs=[];inventory={}
 specs=[('roster','runtime::group::roster_offset_tests::',1),('plan-owner','session::pipeline::plan_owner_tests::',2),('continuation','session::plan_execution::continuation_tests::',2),('knife5b','runtime::drive_tests::knife5b',1),('chain','session::decision_chain::chain_restructure_tests::',9),('atomic','plans::atomic_event_tests::',1),('owner-state','plans::state::owner_tests::',1),('day-end','runtime::day_end_public_views_tests::',6),('admin-group','runtime::group::admin_transaction::tests::',1),('admin-world','runtime::world::admin_transaction::tests::',5)]
 singles=['initial_batch_full_fill_completes_linked_parent_before_later_manual_acceptance','buy_allocation_request_reserves_only_the_next_routable_child','stock_worker_assigns_time_priority_from_supplied_order_not_sealed_identity','restore_rebuilds_remaining_from_live_book_when_field_is_absent','restore_rebuilds_remaining_from_live_book_when_serialized_value_is_stale','restore_rejects_inflated_serialized_remaining_with_fault2185','restore_rejects_live_exposure_beyond_target_with_fault2185','restore_rejects_plan_child_that_does_not_match_a_live_order','strategy_state_identity_tampering_is_rejected']
 for label,prefix,slices in specs:
  cases=[n for n in names if n.startswith(prefix)];assert cases,(label,prefix);inventory[label]=cases
  for i in range(slices):
   chosen=cases[i::slices];assert chosen
   jobs.append((label+'-'+str(i),[binary,'--exact','--test-threads=4',*chosen],repo,10000))
 for name in singles:
  cases=[n for n in names if n.endswith('::'+name)];assert len(cases)==1,(name,cases);inventory[name]=cases
  jobs.append((name,[binary,'--exact','--test-threads=4',*cases],repo,10000))
 (out/'rust-related-inventory.json').write_text(json.dumps(inventory,indent=2)+'\n')
else:raise ValueError(mode)
started=time.monotonic()
with concurrent.futures.ThreadPoolExecutor(max_workers=12 if mode=='rust' else 3) as pool:results=list(pool.map(run,jobs))
receipt=dict(mode=mode,head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip(),parallel_commands=12 if mode=='rust' else 3,wall_ms=round((time.monotonic()-started)*1000),results=results)
(out/('final-'+mode+'-receipt.json')).write_text(json.dumps(receipt,ensure_ascii=False,indent=2)+'\n');print(json.dumps(receipt,ensure_ascii=False,indent=2),flush=True)
if any(r['exit'] for r in results):sys.exit(1)
if mode=='rust' and any(not r['summary'] or any('ok. 0 passed' in s or not s.startswith('test result: ok.') for s in r['summary']) for r in results):sys.exit('缺少有效 Rust 成功用例')
