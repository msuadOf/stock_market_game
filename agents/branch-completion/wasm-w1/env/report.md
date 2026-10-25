# W1-1 env 绑定实施记录

日期：2026-10-26。独占 worktree `.worktree/wasm-w1-env`，分支
`feat/wasm-w1-env-approved`。完整基础为已可信终审的 W0
`a72709ba825ff9b96d1505f8a202f977ae4ad28f`，并非只取末尾证据补丁。
最终只移植 W1-1 增量，不整体 merge 旧基分支；main 组合验证另记。
冻结设计 SHA256：`95f965f759a0adae014752a8205b0dac1fe06f6070d4aa5ac144eeb8b270ddf4`。

## 实现边界

- `engine-clock-env.ts` 保留 clock 原函数，五 import 委派独立 binding 单例。
- 一次 memory/角色绑定，实例化后的实际 layout 一次装入；每次 arena/futex/name
  均以当前 memory.buffer 建 view，不缓存 grow 前长度。
- wait 接实际 i64 BigInt；-1n 无限，其余非负安全整数精确转换。raw i32 经完整
  32 位域校验后还原 u32，expected 高位不丢失，workerId 高位正常保留。
- H5 同步复制并 fatal UTF-8 解码名称，保留 U+FEFF；slot/gen 直接来自实参，
  不读当前槽猜代。terminate 仅发控制消息，保留 postControl 原始错误。
- exit 仅记录冻结的匹配 bootstrap 数值快照，重复/缺失/角色/身份不匹配显式错；
  无成功 exit 消息、close、destroy 调用。
- engine/child/ingress 允许 futex；生产 wait 真实 DedicatedWorker 检测；
  ingress 拒绝 spawn/terminate/exit，只有 child 可记录 exit。
- 未改 wasm-worker、Rust、runtime 开关、supervisor 算法或构建面。

## 红绿与命令

先准备 12 组合同测试。W0 最终可信批准前只运行独立 tiny WASM ABI 测试（1/1），
不以缺 API/import/编译失败计红。W0 通过后先放最薄接口 scaffold，把五 import
转给未改动的 W0 显式失败桩；第二次红测的绑定入口只是 scaffold，不模拟生产
成功，wait/notify/H5/exit 等仍实际因 W0 桩而失败（14 项中 3 过、11 败）。
`red-binding-scaffold.txt` 保留原件，`red-behavior.log` 包含真实业务失败详情。
首次 scaffold 绑定直接 pending 的 `red.log` 也保留，但不单独当行为红证。
实现后 binding/env/protocol/supervisor 定向 36/36 通过；Node25 与精确
Node24.18.0 都通过（green.log、green-node24.log）。

普通测试命令（基础 Node25.8.2；Node24 用根 `.tmp/node24-env/node` 同样替换两处 node）：

```sh
node scripts/run-with-deadline.mjs 10000 -- node --test --test-concurrency=4 --test-timeout=10000 apps/web/src/host/facade-thread-binding.test.ts apps/web/src/host/facade-thread-env.test.ts apps/web/src/host/facade-slot-protocol.test.ts apps/web/src/host/facade-supervisor.test.ts
```

TypeScript 和 oxlint 两个独立检查同时执行，各自进程外 10000ms deadline。
窄 TypeScript 首次遗漏 `--types node --typeRoots apps/web/node_modules/@types`
失败（原日志 typecheck-missing-types.log）；补齐命令参数后通过，源码未为此改动。
窄检查通过日志 typecheck.log、lint.log；所有 4 个代码/测试文件均覆盖。

```sh
node scripts/run-with-deadline.mjs 10000 -- node apps/web/node_modules/typescript/bin/tsc --noEmit --target es2023 --lib es2023,dom --module esnext --moduleResolution bundler --allowImportingTsExtensions --strict --skipLibCheck --types node --typeRoots apps/web/node_modules/@types apps/web/src/host/engine-clock-env.ts apps/web/src/host/facade-thread-binding.ts apps/web/src/host/facade-thread-binding.test.ts apps/web/src/host/facade-thread-env.test.ts
node scripts/run-with-deadline.mjs 10000 -- apps/web/node_modules/.bin/oxlint --deny-warnings apps/web/src/host/engine-clock-env.ts apps/web/src/host/facade-thread-binding.ts apps/web/src/host/facade-thread-binding.test.ts apps/web/src/host/facade-thread-env.test.ts
```

最初草拟 ABI 小测曾因 workdir 拼接错误未生成文件；修正后才运行，不计该命令为红证。
未跑 all、Cargo 或浏览器；Node wait/notify 与真实 tiny WASM 只证明本批边界，
不等于产品 glue 接线、真实浏览器闭环、G+N runtime 或 R4a 完成。

本批不改变 A 股交易制度、资金/股份单位、T+1、受理序、Abort 或日终档语义；
独立 reviewer 仍须审完整 diff 的语义、必要性与边界。

## 独立复核

待未实施者可信复核；不能以本记录宣称 W1-1 完成。
