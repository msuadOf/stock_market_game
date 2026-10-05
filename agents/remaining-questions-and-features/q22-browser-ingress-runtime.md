# Q22 Browser shared-WASM 代表性运行验收

## 目标与运行入口

`q22-browser-ingress-runtime.mjs` 使用真实 Chromium、Vite、生产 `wasm-worker.ts` 与生产
`wasm-ingress-worker.ts`。不使用 FakeWorker，不替换 Rust bindings，不补盖 receipt。
测试只加载一只沪市 MainBoard 股票、零 NPC、30 tick 的压缩交易日；金额为分、委托数量
为股，买入 100 股，T+1 与现有集合竞价/连续竞价规则不变。这是运行协议 fixture，
不是把压缩时间或零 NPC 声称为真实 A 股市场。

```bash
node scripts/run-with-deadline.mjs 10000 -- node agents/remaining-questions-and-features/q22-browser-ingress-runtime.mjs
```

整条命令（含 Vite/Chromium 初始化及清理）由进程外 10000ms deadline 约束，只有一个
代表性 case，不使用长验收放宽上限。启动输出实际 Rayon threads（最多 4，受可用 CPU
限制）、Engine/Intake worker 数。构建不在此命令内；WASM 必须由 root 统一执行多核构建。

## 必须观察的真实事实

1. Engine Worker 创建生产会话，返回 compiled `WebAssembly.Module` 与 `SharedArrayBuffer`
   支持的 `WebAssembly.Memory`；独立 Intake Worker 使用这两项初始化真实 bindings。
2. 外层测试消息使用 `Atomics.wait` 暂停 Engine Worker 的 JS 消息消费。Intake Worker
   完成完整 `PlaceLimit` payload 登记时，gate 尚未释放。这个状态直接读取 Atomics，
   不以墙钟时间或事后消息排序冒充 receiver receipt。
3. 释放 gate 后执行生产 `stepOnce`，核对 tick 1 的真实活动委托：股票、方向、价格及
   剩余数量均与完整 payload 相符。
4. 经生产 `stepOnce` 推进至自然日日终，再从生产 `save` 得到 producer save；生产
   `restore` 成功后，在仍绑定旧 token 的 Intake Worker 上提交，必须由 Rust registry
   明确拒绝旧 token。重新绑定新 token 后，旧 generation 必须在 Intake Worker 拒绝，
   恢复后的活动委托仍为空；新 generation 正常登记后，继续生产 step 必须在 tick 31
   产生唯一的 200 股新委托。

## 明确不覆盖

- `Atomics.wait` 仅证明 Engine Worker 不能消费 JS 消息时，真实 shared-WASM intake
  仍可登记；**不是**生产 `step` 正在执行或慢 NPC 决策计算中的证明。
- 零 NPC fixture 不验证 Browser 的 Player/NPC 同 receiver ordinal 竞争或 tick cutoff
  已冻结后的归属。Native Rust 的跨来源/生产 step 测试是独立证据，不能替代这些 Browser
  运行时事实。脚本结果显式输出 `productionStepBusyRuntimeVerified: false` 与
  `npcCompetitionRuntimeVerified: false`。
- 本脚本是 Agent 工作验证，不自动加入普通生产测试发现，也不取代已有 Playwright
  用户旅程或 Q22 的完整验收/独立 diff 复核门禁。

## 首次实测

2026-10-05，Node v25.8.2、Playwright 1.63.0 与 Chromium 1243 已安装。上述正式命令
在 0.63 秒内红灯：现有 `apps/web/wasm-pkg/web_wasm.js` 缺少 `ingress_token` export。
脚本在启动浏览器前显式终止，未声称任何 shared-WASM runtime 通过。
## 阶段产物实测

Root 重建 WASM 后，首次真实 Browser 执行明确拒绝 harness 的错误 `LimitPrice`：
测试误将 `price` 写为 Money 字符串。只把 fixture 修正为生产 Intent 的
`{ Fixed: "1008" }`，未修改 host 或 Rust，也未弱化完整 payload 断言。

随后两次正式十秒命令均 exit 0。最终一次含启动、执行、清理耗时 **4.13 秒**，真实原始
输出保存在 [`q22-browser-ingress-runtime.log`](q22-browser-ingress-runtime.log)。

- 实际 4 个 Rayon threads、1 个 Engine Worker、1 个 Intake Worker，共享 compiled Module
  与 shared Memory。
- owner gate 未释放时 Intake 登记完成；释放后 tick 1 恰有 100 股、1008 分的 Buy
  auction 委托，Cash 冻结由生产 Engine 计算。
- 生产日终 save 的 tick 为 30；restore 后 token 1 → 2、generation 1 → 2。
- 旧 token 与旧 generation 均明确拒绝，恢复后旧请求未产生委托；新 generation 推进
  至 tick 31 恰有唯一的 200 股新单。
- 日志仍如实输出两项未覆盖标志 `false`；没有据此核销忙 NPC/cutoff/跨来源 Browser
  receipt 竞争或整个 Q22。

日志中的 deprecated initialization parameter warning 来自既有 wasm-bindgen 调用，不是
吞错或 fallback；本批不顺带修改生产初始化接口。

非实施者 `review_shared_ingress_core` 已独立只读复核脚本、说明和原始日志，未发现必须
修复项：A 股单位与压缩 fixture 边界一致，gate 没有用超时释放冒充独立受理，恢复后
唯一新单排除旧请求污染，未夸大忙 step/NPC/cutoff 证据。复核者未重跑 Browser，运行
结果仍以本节真实工具输出为依据；本项增量通过不替代父任务完整 diff 门禁。

## 最终源码产物复跑

root 在 registry ownership、receipt fixture 与 verification-only 修复稳定后，使用 pinned
`nightly-2026-09-05`、jobs 32 / Rayon 32 与外部 300000ms deadline 重建最终源码的正式
WASM release，成功耗时 **59.78 秒**，日志为
`../../.tmp/checklist-wave3/shared-ingress-wasm-final-build.log`。

随后同一正式十秒 Browser 命令 **exit 0**，输出原样归档为
[`q22-browser-ingress-final-runtime.log`](q22-browser-ingress-final-runtime.log)，完整日志为
`../../.tmp/checklist-wave3/browser-ingress-final-runtime.log`。此次产物对应最终源码，
不是将阶段产物的 4.13 秒结果外推为当前版本验证。实际 4 个 Rayon threads，
Engine/Intake Worker 各 1；tick 31 唯一新代 200 股 Buy，旧 token/generation 显式拒绝，
full payload、shared Module/Memory 与 owner gate 断言全部通过。

最终日志仍输出 `productionStepBusyRuntimeVerified: false` 与
`npcCompetitionRuntimeVerified: false`；不覆盖忙碌 production step/NPC/cutoff 跨来源竞争，
不核销整个 Q22。阶段 4.13 秒日志独立保留，不混淆 source 对应关系。
