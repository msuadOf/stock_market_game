// 冻结实际源码输入；agents 工作证据另行封存，不混入构建身份。
import fs from 'node:fs/promises';
import {collectFullRegressionSourceFingerprint} from '../../.worktree/plan-identity-combined/scripts/run-full-regression.mjs';
const repo='/data1/baiyifan/workplace/stock_market_game/.worktree/plan-identity-combined';
const result=await collectFullRegressionSourceFingerprint(repo);
await fs.writeFile('/data1/baiyifan/workplace/stock_market_game/.tmp/plan-identity-combined/source-final-after-tests.json',JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify({files:result.files.length,digest:result.digest}));
