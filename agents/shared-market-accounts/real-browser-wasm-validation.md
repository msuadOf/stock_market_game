# 真实 normal WASM 与浏览器 IndexedDB 短验证

## 验证路径与资源

工具：`real-browser-wasm.test.mjs`。本机 Chromium、Vite 静态服务提供 COOP／COEP；两个隔离
browser context 分片并行，每个真实 Module Worker 初始化 Rayon 2。使用现成 normal
`apps/web/wasm-pkg`，没有 Cargo／WASM 重编译或生成 JSON 手工修改。

每个分片执行：真实 fixture → 当前严格 Save parser／日终校验 → Worker `restore_json` →
`prepare_public_baseline` → `snapshot`／`save` → 当前严格日终候选校验 → 真实 IndexedDB
save／list／copy／select／load → 第二次 Worker restore／snapshot 等值。该路径验证载入后的
日终候选保存，不证明下一次自然日自动日终生产、全 Host 或真实多人 E2E。

normal 二进制由正式 release export checker 检查不含 NPC 私有诊断导出，Worker 再检查
`host_capabilities().npcDecisionDiagnostics === false`；fixture／WASM SHA 在前后检查。
单 case 与进程外整命令树 deadline 均 10000ms，资源清理分别尝试并聚合失败。

## 已执行结果

首次旧 fixture 的真实日终校验拒绝未处理 NPC 请求，不能计为通过。探索工具还曾错误调用
main-thread／对象 serde restore，产生 Atomics.wait／i128 输入错误；这不是生产 Worker 的
`restore_json` 路径，已改为真实 Worker，不冒充根因或修复证据。旧失败日志保留在
`.tmp/real-browser-wasm-game-candidate-rejected.log`。

日终 NPC 根修后，Root 重新生成 normal WASM，真实 ProtocolSession producer 安装有效
日终候选；Native Protocol restore→save 深等值及 Node 严格日终校验由 producer 作者确认。

- fixture：`apps/web/src/save/fixtures/current-schema-save.json`
- fixture SHA：`f81dc44c86912666e9c78fbe2af541c7c36448d60c33ce2b82b9686442db43fb`
- normal WASM SHA：`b319e641bacd2193be5173f6f05f92b731a1796854fc09095e8a634a1ef6aad4`
- tick 120，current date 2030-01-09，settled through 2030-01-08；pending NPC intent 为空。
- 正式命令：`node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=2 agents/shared-market-accounts/real-browser-wasm.test.mjs`
- 实际结果：两个隔离分片均失败，外部命令约 6.58 秒、exit 1，SHA 前后未漂移。
- 错误：`invalid save: NPC 7 attention probability 0.03979332740595576 does not match its deterministic profile 0.03979332740595565`。
- 日终待处理请求错误已消失；新失败位于 Native→WASM 的确定性 profile 浮点重建校验，不是 IndexedDB 写入失败。实际未到 IndexedDB save，因此不声称真实 IDB 成功路径通过。
- 完整日志：`.tmp/checklist-wave4/real-browser-wasm-day-end-current.log`。

不降低断言、不清空 pending、不改 seed／setup、不自修真实 JSON，不用另一制品偷偷绕过。
新根修若影响 WASM，必须由 Root 正规重建 normal 包后再绑定新哈希重测。

## 独立复核

脚本由未实施者完整静态复核；两项工具 P2（normal 制品检查、失败资源清理）已修复并复核。
记录见 `real-browser-wasm-independent-review.md`。新真实跨平台红结果已经交 Root 与 fixture
producer；静态复核不能替代真实绿色验收。

## 数学根修后的真实绿色

Root 修复跨平台 probability 重建公式后，重新实际生成 Protocol 日终档并实际重建 normal
WASM；收到新制品明确授权后才运行，不使用此前失败包。

- fixture SHA：`5457be5b68eba83576a0a87f4f54b8123d119a252543b7e49721924977a5fcb1`
- fresh normal WASM SHA：`36210175fa7a3152dea84ff407f6be3f53813aef2cf2653b8c359105aef9a58f`
- 正式同一 10000ms 外部进程树／case deadline 命令实际 exit 0，wall 约 6.93 秒。
- 两个隔离 browser context 同时运行，各 Worker Rayon 2；case 约 5.72／5.68 秒。
- 两分片均实际执行 Worker restore_json／prepare_public_baseline／snapshot／save，实际候选通过严格 parser／日终校验，真实 IndexedDB save／list／copy／select／load 提交成功，再 Worker restore 后 snapshot 等值。
- `committed=true`、`selected=true`、`sameSnapshot=true`；tick 120、seq 1473、本人公开账户 `0`、settled through 2030-01-08、两个数据库槽。
- fixture／WASM 双 SHA 前后守卫通过，normal 私有诊断导出／capability 检查通过。
- 日志：`.tmp/checklist-wave4/real-browser-wasm-probability-current.log`。
- Node 显示 pass 3 包含一个父测试，实际独立隔离分片是两个，不能称为三个场景。

该绿色证明上述真实当前固定候选的 Worker load／save 与 IndexedDB 显式存取路径；没有推进
新的自然日，不宣称新的自动 EOD 生产、全部 Host、全回归或真实多人 Server E2E。旧失败证据
完整保留，新绿色结果已送未实施者亲读复核。
