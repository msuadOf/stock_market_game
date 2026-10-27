// 核心身份限域回归：每个子命令外监督10000ms，独立套件并发执行。
import fs from 'node:fs';
import {createHash} from 'node:crypto';
import {runBoundedCommand} from '../../scripts/run-with-deadline.mjs';
const target='/data1/baiyifan/workplace/stock_market_game/.tmp/plan-identity-target/debug/deps/';
const unit=target+'engine-f7ff6b8af6493b4b';
const artifacts=fs.readFileSync('agents/branch-completion/plan-identity-reviewed-build.jsonl.log','utf8').trim().split('\n').map(JSON.parse);
const orderbook=artifacts.find(r=>r.reason==='compiler-artifact'&&r.target.name==='orderbook'&&r.executable).executable;
const jobs=[
 ['behavior',target+'plan_identity_behavior-cd7d4f843225f801',[]],
 ['identity',target+'plan_identity-86432f3ab86f68c2',['--skip','export_bindings_plan_identity']],
 ['plans',target+'plans-64840d2b064df8dc',[]],
 ['allocation',target+'plan_allocation-054e59509e2fbd99',[]],
 ['audit',target+'audit_restore_guards-df490e528cb6122b',[]],
 ['save-contract',target+'save_contract-48e709b7c058a643',['restore_guards']],
 ['orderbook',orderbook,[]],
 ['plans-unit',unit,['plans::atomic_event_tests']],
 ['state-unit',unit,['plans::state::owner_tests']],
 ['continuation-unit',unit,['session::plan_execution::continuation_tests']],
 ['initial-unit',unit,['initial_batch_full_fill_completes_linked_parent_before_later_manual_acceptance']],
 ['allocation-unit',unit,['buy_allocation_request_reserves_only_the_next_routable_child']],
 ['stock-fifo',unit,['stock_worker_assigns_time_priority_from_supplied_order_not_sealed_identity']],
];
const names=fs.readFileSync('agents/branch-completion/plan-identity-unit-list.log','utf8').split('\n').map(l=>l.replace(/: test$/,''));
for(const name of names.filter(n=>/::(restore_rebuilds_remaining_from_live_book_when_field_is_absent|restore_rebuilds_remaining_from_live_book_when_serialized_value_is_stale|restore_rejects_inflated_serialized_remaining_with_fault2185|restore_rejects_live_exposure_beyond_target_with_fault2185|restore_rejects_plan_child_that_does_not_match_a_live_order|mid_day_termination_cancels_the_in_flight_child_first|mid_day_termination_removes_a_linked_parent_without_an_active_child|ordinary_adoption_rejects_exceeding_remaining_quantity|ordinary_adoption_within_remaining_passes|failed_candidate_rolls_back_adoption_without_residue)$/.test(n))) jobs.push([name.split('::').at(-1),unit,[name,'--exact']]);
const manifest={source_commit:'07349845665609813394ae1d84f8b922b9e27da3',deadline_ms:10000,parallel_commands:jobs.length,test_threads:8,results:[]};
const hashes=new Map(Object.entries(JSON.parse(fs.readFileSync('agents/branch-completion/plan-identity-binary-hashes.json','utf8'))));
const results=await Promise.allSettled(jobs.map(async([label,command,args])=>{
 const log=`agents/branch-completion/plan-identity-reviewed-${label}.log`;fs.writeFileSync(log,'');
 const actualArgs=[...args,'--test-threads=8'];
 const binarySha=hashes.get(command);
 const start=performance.now();
 const result=await runBoundedCommand({command,args:actualArgs,timeoutMs:10000,captureOutput:true,
 env:{...process.env,TS_RS_EXPORT_DIR:process.cwd()+'/apps/web/src/types/generated',TS_RS_LARGE_INT:'number'},
 onStdout:c=>fs.appendFileSync(log,c),onStderr:c=>fs.appendFileSync(log,c)});
 const summary=result.stdout.match(/test result:[^\n]*/g);
 if(!summary||summary.some(s=>!s.includes('test result: ok.')||s.includes('ok. 0 passed')))throw new Error(`${label}: 缺少有效成功用例`);
 return {label,command,args:actualArgs,binary_sha256:binarySha,elapsed_ms:Math.round(performance.now()-start),summary,log};
}));
for(let i=0;i<results.length;i++){const r=results[i];const value=r.status==='fulfilled'?r.value:{label:jobs[i][0],error:r.reason.stack};manifest.results.push(value);console.log(JSON.stringify(value));}
fs.writeFileSync('agents/branch-completion/plan-identity-reviewed-results.json',JSON.stringify(manifest,null,2)+'\n');
if(results.some(r=>r.status==='rejected'))process.exitCode=1;
