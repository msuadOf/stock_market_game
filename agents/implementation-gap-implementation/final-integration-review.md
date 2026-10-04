# 最终跨层整合独立复核

## 复核身份与固定范围

- 日期：2026-10-04；非实施者 `/root/review_final_integration`。只审查及定向短验证，不实施产品、不 stage、不 commit、不运行完整回归。
- 工作树：`.worktree/implementation-audit-final`；观察时 HEAD 为 `b6dbb9f405b86ec6983550ab12ac96d1f1e2b34c`。固定快照为 `.tmp/final-gap-review/remaining-v2.patch`，6156 行、445118 字节，SHA-256 `6b18e1920153509b537aa9ba76412c9dc3805056ae47c1df6b4d74417071c6e0`。
- 已完整读取上述 diff 至 EOF，包括追加的未跟踪产品源码；两条约 65KB 的 minified JSON 使用连续字符区间补读，前期工具截断未当作全文通过。自动生成的 `packages/engine/bindings/` 不属于固定产品快照；正式 Web generated 文件包含在审查范围。
- 已读 AGENTS、principles、architecture、open-questions，以及 `session-integration.md`、`web-save-contracts-review.md`、`personal-strategy-review.md`、`company-assembly-review.md`、`retail-beliefs-review.md`、`tick-performance-review.md`、`report-scope-policy.md` 和 checkpoint 定向定位记录。已提交批次由对应独立 reviewer 审查，本记录不追认重新审过所有旧批次。
- 初读总账仍为已核销 63 / 待登记 16，不能据此断言还有 16 项没有实现，也不能预先宣告全部 79 已核销。最终总账增量另行核收。

## 跨层语义与必要性

1. Session setup 的可省略 `company_operations` / `groups` 与 Rust serde default、Web exact parser、正式生成类型保持一致；SaveSlot 根 groups 必填。恢复比较 groups/setup、全部经营公司 keys/spec，并允许未上市自定义公司；没有以默认 Industrial 冒充四行业。
2. Industrial 保存 trade/inventory source 身份而非冗余派生金额；直接 typed SaveSlot 与 serde 的身份 guards 对齐。PaymentFailure 为当日零幅度付款事实，不能注入 active shocks，正会计金额以 i128 分范围校验；贷款 maturity_date 保留真实到期日，合法逾期不改日期。会计金额两位小数字符串与股票 Money 分、qty 股没有混换。
3. Consolidated 缺归母 NI 在 source、Closing 恢复和直接 SaveSlot 中拒绝；估值事实另有 typed unavailable。Consolidated CF 归母归属不足明确不可用，不按持股比例猜分摊，也不回退到其他估值方法。Standalone 合法 None 不被误拒绝。
4. Institution / Retail 共用本人已知报告 selector：期间优先、同期间 Consolidated 优先、同 scope 版本优先；未获知信息不参与。中期同比与全年盈利基数保持同 scope。ADR-0016 明确这是 game policy，不是 CAS 33 强制投资估值制度；官方核验记录与该表述一致。本 reviewer 未重新联网取得法源，不宣称完整 CAS / A 股拟真合规。
5. Retail 个人分析经真实 capture 封存为 P2 输入，继续使用 Retail 执行器，未混入 Institution policy、parent execution 或账户暂停 latch；P2 保留风险、T+1、低信心、冷静期与未随机到达的优先约束。dated influence 不删除真实经历。
6. memory 保护集为持仓与 active plans；Root/Lifecycle 后 AccountExecution 修剪、新计划保护、P9 实际 receipts 影响账户修剪、日终全部个人账户收口相互衔接。未加入逐 tick 全账户扫描或新市场配额；剪除历史关注不删除已知材料。
7. Arc history 的 new/restore/save、shadow、公开披露及 closing 写入口一致，mutation 使用 make_mut，PlanBook 持久化 root 的跨批实现由独立性能 reviewer 核收。诊断 decimal string 不改变 Rust 数值计算或正式委托顺序；trace plan_changes 与 Web whitelist、SSR 同步。
8. Server actor fixture 仅补 `company_operations: None` / 空 groups，必要且不修改现有断言。已独立读 `.tmp/final-server-actor-build.log`：targeted actor no-run 在 44.00 秒成功；不是全部 Server / Desktop / 三宿主编译通过。

当前完整固定产品 diff 未发现新增 must-fix 跨层缺陷；三门禁在上述限定范围内通过。Q11 更正分发、Q17 Closing 完整版本/重述不变量及 Q25 双 profile 契约没有被这批窄修复擅自核销。

## Reviewer 亲跑短验证

- Web：`node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=8`，六文件为 company-contracts、company-books-schema、personal-schema、personal/beliefs、npc-decision-trace、npc-decision-inspector。6 文件通过 / 0 失败，Node duration 581.77ms；此为文件级汇总，不冒充内部 case 数或完整 Web 回归。
- Rust 固定 binary `.tmp/personal-final-engine-tests`，SHA-256 `4940159ec290d40adaeb740cbb7c84a2091b1df978c7635730fbccd69e9a20cf`。先实际 list 确认目标 case，binary 共 1150 case；四命令并发，各 `RAYON_NUM_THREADS=8`、`timeout -s KILL 10s`、`--exact --test-threads=8`：
  - `retail_restore_rejects_institution_account_risk_pause`：1/1，0.29 秒。
  - `direct_save_slot_rejects_missing_consolidated_parent_income`：1/1，0.39 秒。
  - `ordinary_tick_shadow_shares_report_histories_and_isolates_mutation`：1/1，0.18 秒。
  - `completed_sell_prunes_memory_before_quiet_point_save_validation`：1/1，6.39 秒。
- 未重复共享构建；上述固定 binary 与本人运行结果不等同于当前源码的全仓编译或所有用例通过。未声称实际 CPU/线程利用率已取证。

## 保留边界

- 旧 Retail 70/1、fundamental 23/26、early binding timeout 和旧 binary 的 0 case filter 仍是失败历史，以最新独立实测替代当前阻断，不删除、不追认。
- 初批部分 TDD 只有测试先落盘但没有正确原因 behavior-red，属于既有流程偏差；最终 green 不修复历史流程事实。
- 9 股接受侧仅验证完整生产 SaveValidation guard，不宣称完整 restore 后续装配；小 fixture restore、9 未保护拒绝及真实清仓另有实证。
- 旧压缩人工 save fixtures 只支持 Web 结构 parser，不作为 Rust 全恢复合法性或公司账源历史证明；真实四行业 / mixed group / internal-sale-group bridge 证据以公司与 Web 独立复核记录为准。
- closing expiry 日内低层 SaveSlot checkpoint 定位保持独立边界，不扩大成公共日终档失败，也不把 fixture 改选合法锚点当作生产修复。
- 完整回归、长期经营统计、browser matrix、所有三宿主构建、长期吞吐与峰值资源验收均未执行。

## 最终总账增量签核

本节替代前述初读 63 / 16 的当前状态，但保留当时观察事实。产品分组提交已到 `0f79045311f6cd83f5d78f500c55d680dd2f30bf`（个人策略 `aebfe39`、共享存档 `fabbd41`、诊断 `0f79045`）；本次没有新增业务 diff，也没有重跑回归。

- 已完整读取 [总账](../implementation-audit/implementation-audit-2026-10-02.md) 与 [实施索引](README.md) 的最终全部 diff，逐个对照最后 16 项及相关专项复核的最终证据。总账 SHA-256 为 `1173825ba50db3ac98c485e3a09010253b10c8b17b2eeb04d9e942a2c7e5ce8e`，README 为 `a49bfb48a4b88baf0e2a9dba599b2d512705c38863a62a96d2f4b7ccc7af8bdd`。
- 只读程序独立核对总账 §2：79 行、79 个唯一 G、全部有“已补齐”标记，已核销 G27 不在现行集合；确认 G 剩余 0。总账及本主题 44 份 Markdown 的相对链接共检查 45 文件，缺失目标 0；两文件 diff-check 通过。
- G42 明确“8 条 / active 保护接受 guard”而不是 9 股完整 restore 装配；G39 保留默认 12 tick fixture、完整矩阵未运行；G16 只签局部 COW / history 共享及隔离，不冒称长期吞吐。G09 明确 scope 是 game policy，归母 NI / CF 与集团总量不混。G35 的 25 case 为原 23 加两条真实非空行业公告，和最终实施/联合复核一致。
- 已完整读取 [parent checkpoint 定位](parent-checkpoint-expiry.md) 与其 65 行 probe，并核收实际日志 1/1、1.80 秒。probe 先实证 closing 日内低层 checkpoint 拒绝，再经真实收盘及自然日日结验证公共档无 parent / resting / auction orders，公开 restore 后 JSON 全等；它不是生产修改、不新增 G、不宣称公共日终档损坏。原 G39 机构 fixture 联合 guard 的精确失败条件没有被此不同最小 fixture 冒充全部证明。
- Server actor 的三项 speed 过滤短测 3/3、0.74 秒、Rayon 8 / test threads 8 / 外部 10 秒为 root 执行报告；本 reviewer 未重复执行，且不据此声称全部 Server 套件通过。44 秒 targeted no-run 的独立日志核收保持前节范围。

**最终裁定：79 项确认 G 的限定实施与独立复核登记通过，确认 G 剩余 0；无新增必须修复的总账或跨层发现。** 静态审计基线与后续实施状态已分开，Q、未来产品、法源受阻及未执行的浏览器 / 跨平台 / 长矩阵、未知缺陷边界均保留。本结论不等于全仓无缺陷、完整回归、正式发布或真实 A 股全面合规。
