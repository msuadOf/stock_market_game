# 独立总复核初始边界

## 身份与状态

- Reviewer：`/root/implementation_final_review`；未参与源码、测试或实现说明的实施，只编写本目录审查文件。
- Branch：`refactor/oop-complete`；基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 当前状态：实现进行中，尚未收到 root 的 final 冻结通知；本记录不是完成签署。
- 本轮不执行 Cargo、测试、构建、生成器或 Git 写命令；只读 Git 用于取得完整 diff 和基线对照。不创建其他 subagent。
- 原有 `AGENTS.md`、`.worktree/`、`agents/oop-refactor-audit/` 只读保护；冻结时核对 protected-baseline 与完整范围。

## 需求及核销规则

权威正文为 audit 的 `challenge-2026-10-03/action-index.md`，不把 assigned-actions 内历史增强与现行正文冲突当作新需求。清单机械对账为 128 个唯一 ID：Domain 39、Pipeline 20、Session 19、Frontend 16、Hosts 34。六项本轮增强与四项历史增强不重复计数，但逐项纳入所属动作。用户已授权所有 optional 目标，不能用低优先级省略。

`action-ledger.json` 当前由权威清单与 assigned-actions 机械建立，只证明 ID 完整覆盖，**不证明目标实现或审查通过**。冻结后每个 ID 需要：

1. 原目标字段/共同不变量/资源的实际 owner；具名方法的完整实现位置；没有原状态外另立可写权威。
2. 全部生产 caller、恢复投影和必要 fixture 的真实迁移；不是加 facade 后保留原写法。
3. 被替代字段、refs、旧函数或公开写口的撤回证据；现有 API 接受范围不得因收紧改成新行为。
4. 原 guard、首错、checked 运算、部分写入、panic/返回错误、serde 扁平形状、空值/额外键接受范围、浮点运算与 RNG 次序的行为保持证据。
5. 与风险对应的短测试证据：命令、case/文件、源码版本、退出状态、耗时、并发与 deadline；未运行或失败如实标注。baseline 保护测试通过不冒称 TDD 行为红；缺 API 编译红不冒称行为失败。
6. 具名未实施者对所属完整 diff 的独立审查记录，包含实际阅读范围、基线/冻结版本、有效发现及修复后复核。Root 与 manager 有实施，不充当独立 reviewer。
7. 本 reviewer 对跨层关键接缝亲读的补充证据；组级全 diff 承接与本人的实际亲读分别披露，不宣称我个人读完全仓。

## 跨层重点

- `AccountBook → Account → Position`：页面 COW 与校验缓存失效、T+1 锁定、分/股单位、恢复与 strategy patch；不给生产任意 setter。
- `Order/OrderBook/ParentOrderPlan/TradingPlan`：各自事实与转换唯一归属；实际同股受理与价格时间优先不被 identity、输出排序或 treap priority 改写。
- P0/P1/P9：固定 post-P0 资源不被到期汇总、成交或释放二次补入；跨轮 fact 消费与最终结算不重复；候选提交和失败回滚分离。
- `CommittableSessionState`、`ProtocolState`、`CivilUpdate` 与公共 SaveSlot：唯一 state 表示、checkpoint/rollback、publication facts、日终保存 gate 保持；内部活动 envelope fixture 不扩张公共读档接受范围。
- 四行业账套/个人经历：公司资金、投资者账户和个人信息边界保持；行业 guard/preview/post/apply 和已有失败面不暗改；pause 不强制卖出，目标数量不冒充可负担、受理或成交。
- 宿主及 UI：`EngineHost/HostUpdate` 契约保持，Rust registry、WASM slot/timer、Worker request、Remote publisher/command/report、Tauri timeline 各保留实际资源语义。React/Redux 不另立行情或查询权威。
- 工具与 fixture：对象必须拥有成套事实或真实生命周期；断言与负向输入不藏进自动修补 helper；deadline、并发、inventory 接受集合和清理保证不扩张。

## 测试及领域结论的限制

用户要求成熟实现后仅针对性短单元测试及必要编译。本复核不要求、也不运行全回归、E2E、长期性能矩阵；不能以未运行长验收直接否定本轮范围，不能把短测试写成长期/跨宿主验收通过。普通测试 command 与 case 都受 10000ms 上限，适用 Node 同时具备 case timeout 和进程树 deadline，并行预算须真实。

本轮目标为行为保持 OOP 重构，领域依据承接现有 ADR 与 trading-rules 的明确简化/blocked 边界，不重新认证现实制度全部原文。发现新增交易制度、税率、拒绝范围或清理保证时，作为未授权行为变化报告，不靠此次重构补齐它们。

## 已读与待读

已亲读：AGENTS、principles、architecture、testing、open-questions、trading-rules、ADR-0010、ADR-0025、audit summary、现行 relationships、implementation README、三份现有 coordination/request-owner 记录，以及权威 action-index 的 Account A03 全文与其历史增强。五组 assigned-actions 的完整 ID/标题机械对账已完成；其约 1MB 详细正文尚未全量亲读，不能写成 EOF 阅读完成。冻结前及总复核中继续按动作与关键接缝逐段读权威正文；最终报告明确个人亲读范围，其余承接具名组级独立完整 diff 证据。

冻结后先取得完整 tracked+必要 untracked diff 范围及源指纹，覆盖所有变化路径，再核 128 项证据、组级 review 和关键接缝。任何有效发现同时发所属 manager 与 root；修后对变更段及受影响关系再审，不提前形式签署。
