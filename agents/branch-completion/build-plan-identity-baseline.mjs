// 冻结旧源码负控：只复用第三方依赖，不修改旧源码或使用新实现回填。
import {runBoundedCommand} from '../../scripts/run-with-deadline.mjs';
const root='/data1/baiyifan/workplace/stock_market_game/.tmp/plan-identity-target';
const deps=root+'/debug/deps';
const args=['--edition=2021','--crate-name','engine','--crate-type','rlib','--emit=link','-C','debuginfo=0','-C','codegen-units=16','/data1/baiyifan/workplace/stock_market_game/.tmp/plan-identity-red-source/packages/engine/src/lib.rs','-L','dependency='+deps,'-L','native='+root+'/debug/build/tikv-jemalloc-sys-b119d4d03c3244f0/out/build/lib','-L','native='+root+'/debug/build/psm-afc2b2a6dbdaba36/out'];
for(const [name,hash] of [['num_bigint','cc54680ff1ef0d16'],['serde','baaece516c68beee'],['serde_json','64d03d242a680cbe'],['thiserror','ba5f344727a28423'],['tikv_jemallocator','91ebcada332b75e5'],['ts_rs','6cad7aaecaa64faf']])args.push('--extern',`${name}=${deps}/lib${name}-${hash}.rlib`);
args.push('-o',root+'/libengine_baseline.rlib');
await runBoundedCommand({command:'rustc',args,timeoutMs:300000});
