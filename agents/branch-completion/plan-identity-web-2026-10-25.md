# PlanId 身份前置：Web 适配记录（2026-10-25）

## 范围与契约

- 工作树：`stock_market_game-plan-web`；基线 `f5820291`。仅修改 Web 非生成文件。
- 冻结契约由核心 writer 确认：`PlanId={owner,sequence}`，两项均 canonical u64 十进制字符串；`OwnerSequence={owner,next_sequence}`，游标为 canonical u64 字符串或必填 `null`；`PlanBook={policy,owner_sequences,plans}`，两集合均数组。
- 本轮不改撮合、价格时间优先、费用、T+1、资金或股份规则；只将持续计划身份的 wire 表达与 Web 边界对齐。新旧 wire 不兼容是已批准的当前格式决定；生产不提供迁移、alias 或 fallback。
- `TradingPlan.account` 和 `NpcDecisionTraceRecord.account` 同步校正为字符串：`packages/engine/src/orderbook.rs` 的 `AccountId` 已使用 `canonical_u64_decimal` serde；`plans/state.rs` 和 `diagnostics/decision_trace.rs` 对应字段均直接使用 `AccountId`。这是消费既有真实输出，未在 Web 单独改变协议。
- Web 检查完整 PlanId、owner/account 一致、重复完整身份、重复 owner 游标、缺游标、游标不大于已有 sequence、同账户同股非终止计划冲突。会话名册/执行引用等完整领域验证仍由 Rust restore 负责。
- UI 用 `owner=…, sequence=…` 展示，不经 `number`。源诊断仍保留字符串 plan_changes/budget_constraints 原文。

## TDD 与验证

- 先调整计划契约 fixture 并新增身份边界测试；旧 parser 因仍要求 `next_plan_seq`、数值 `plan_id` 而红灯。
- 再新增诊断身份边界测试；旧 parser 因数值 account/plan_ids 而红灯。
- 六个聚焦测试文件共 62 case，通过。命令外 `run-with-deadline.mjs 10000`；Node `--test-timeout=10000 --test-concurrency=8`；wall 约 0.74s。
- 当前存档 fixture 的其他调用方共 89 case，通过。同样 10000ms 进程树/case 上限，8 并发；wall 约 1.52s。
- 代表性大存档 5 case 通过，同样 10000ms 上限；单文件主体顺序执行约 3.44s，独立于其余 fixture 测试运行。原 9.8MB JSON 是历史 schema 语料；只在 test-only `representative-save-test-fixture.ts` 投影新身份和游标，不作为真实 Rust save/restore 见证。
- 一次 fixture 调用方验证从仓库根目录启动，Vite 使用 cwd 导致 `/src/save/save-file.ts` 无法加载；从 `apps/web` 重跑 89 case 全部通过。没有用失败结果冒充通过。
- 受影响 Web 路径 oxlint 无警告；`git diff --check` 通过。
- 一次 app TypeScript 短门限探测触及 10000ms，被外 supervisor 终止。尚未收到新 bindings，此次不作为有效类型检查。

## 尚待前置门禁

- 核心 writer 真实生成 bindings 后，以真实类型做 app/node 编译验证。
- Rust 真实新格式短 save 含计划，经 Web 严格解析并核对 roundtrip；测试 projection 不能替代此证据。
- root 安排未参与实现的可信 Sol 对完整核心/runtime/Web diff 独立复核。未经该复核，不宣称身份前置完成。
