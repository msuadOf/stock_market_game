# G73：Insurance 合同组恢复不变量

## 范围与依据

- 实施 worktree：`.worktree/implementation-audit-final`。本记录不修改总账；限定短测与独立复核已完成，不代表完整回归。
- 已读根 `AGENTS.md`、`docs/principles.md`、相关 ADR-0016/0019/0025、`docs/open-questions.md`、`docs/architecture.md`、`docs/testing.md`、`docs/error-handling.md`，以及 `docs/company-accounting.md` 的 Insurance GMM、适用窗口和 UnsupportedContract 说明。
- G73 证据：总账第 G73 行、`hidden-review/candidate-resolution-01.md` 的保险恢复裁定，以及 `insurance/behavior_tests.rs` 的损坏 released_revenue 导致 post 后 apply 局部失败断言。
- 领域基线沿用正式文档已核验的 CAS 25（2020）与 2026-01-01 其他企业适用窗口；本次不新增交易规则或精算模型，也未重新联网核验法源。金额仍为 AccountingAmount 的 i128「分」，责任单元仍为实际自然日。
- 恢复校验只拒绝对象结构和内部经营不变量不一致，不证明资产来自完整历史，不要求全部合法 Insurance 操作强事务，不处理 Q17 ClosingEngine 原子性。

## 方案

- 保留 ContractGroupState 原有扁平序列化字段；Deserialize 完成构造后调用独立 validator。
- 校验保障期限/责任单元进度、非负余额、保费收讫不超过应收、CSM 与亏损成分互斥、赔案支付范围、五类 FractionUnits 余数边界，以及 GMM 组件与累计调节恒等式。
- 允许合法负余数、重估的有符号 finance/loss 累计，以及期末失活 CSM 的历史余数；不凭印象要求所有期末 carry 清零。
- 提供 InsuranceBooks::validate_restore 供完整 SaveSlot validator 复核直接内存输入，由 root 在 persistence.rs 接线。行业装配 agent 的改动不改变保险存档字段。
- 既有 apply 溢出局部失败测试保留完整断言，改用私有 measurement 直接注入损坏事实，避免把应拒绝的外部 serde 当作内部故障注入口。

## TDD 与验证状态

- 已先写 6 个恢复短测试；首次 red 构建被并行接线阻断：SessionSetup 缺 company_operations 字段，diagnostics 的 DecisionReason match 缺 PersonalAnalysis 分支，后续并行接线也出现新编译阻断；没有把编译失败称为预期 red。
- 为避免改变其他 agent 的半成品接线，在 `.tmp/insurance-restore-red` 从 HEAD 提取独立 engine crate，覆盖本任务测试与实现。隔离 red 已真正运行：6 项中 5 项因 serde 接受损坏对象而失败，1 项合法重估通过，执行 0.00s。MAX 通过 AccountingAmount 自身 serializer 生成，避免把内部「分」整数字符串误当成 JSON「元」字符串。
- 最初隔离构建误复用 `.tmp/gap-target`，同 hash 的 lib binary 因而被 HEAD 快照覆盖，不能被其他主题当作当前生产树的产物。已向 root、Session 接线与 tick 实施者明确通报并停止相关构建，改用独立 `.tmp/insurance-red-target`，不再占用共享 Cargo lock。隔离构建以 `CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0`、`-j16` 和外部 300000ms deadline 准备；隔离结果始终不能替代当前完整树 green。
- 构建命令：`flock /tmp/stock-market-gap-cargo.lock timeout --kill-after=2s 300s cargo test -p engine --lib --no-run -j16 --target-dir .tmp/gap-target`；日志 `.tmp/insurance-restore-build-red.log`。
- 测试执行使用已编译 binary、`timeout --kill-after=1s 10s` 和 `--test-threads=8`，不运行完整回归。
- 首轮独立复核发现合法 MAX 保费可能在组件求和时产生可消去的中间 i128 溢出；新增的组内 MAX 一致状态与真实 establish_group(MAX) 两项短测均确认 red（0.00s）。已修正为固定项先正负 magnitude 消去，再仅对剩余同号项求和；净额确实超出 i128 仍显式拒绝，不增加依赖或限制合法保费。
- 独立二次静态复核已确认根因修复；另补齐 reviewer 建议的偶数责任单元正负 half tie，以及 signed_sum 的 MIN、真实正负净额溢出、可消去 MAX 边界。隔离阶段当时未完成完整树 green；最终 production 结果见下文，隔离结果本身不代表整树验收。
- 独立目标最终短测：`timeout --kill-after=1s 10s .tmp/insurance-red-target/debug/deps/engine-f5de19e1a68767a0 company::insurance --test-threads=8`，22/22 通过，case 总执行 0.01s。5 个 G73 实施/测试文件与临时 crate 逐一 cmp 完全一致；binary SHA-256 为 `e29fc8ab3f544b9ceb421671a5049efb093173e3eac214bc8320c25528f71d0e`。
- 隔离阶段最终可用日志为 `.tmp/insurance-restore-build-isolated-final.log`（缓存准备 0.09s）与 `.tmp/insurance-restore-isolated-final.log`。此前中断后的并发 log writer 造成旧 isolated-green 构建日志夹带 NUL，不用它证明单次构建时长；已完成后重新准备并运行，最终日志无竞争。
- 独立审查已完成三轮，见 [insurance-restore-review.md](insurance-restore-review.md)：首轮发现并修复合法 MAX 中间溢出，二轮确认产品 diff 与 persistence 接线，第三轮确认新增 math 极值断言及记录同步。`git diff --check` 对本任务范围通过。未运行完整回归，未修改总账，未 stage 或 commit。

## 恢复接线

root 的 Session 接线 agent 已在 `validate_company_domain` 的 company 循环中追加：

```rust
if let Some(insurance) = company.books().as_insurance() {
    insurance.validate_restore().map_err(|error| {
        SessionError::InvalidSave(format!(
            "saved company {id:?} has an invalid Insurance state: {error}"
        ))
    })?;
}
```

此调用覆盖直接构造 SaveSlot 与正式 restore；JSON decode 的组内拒绝由 ContractGroupState::Deserialize 执行。

## 当前生产树的最终恢复闭环

- 新增 `insurance/session_restore_tests.rs` 的真实短 fixture：1 只 Insurance 股票、0 NPC、0 新单、quiet shocks，按真实公司装配生成前史并完成休市日结。取得合法 SaveSlot 后，通过公开 establish_group 建立一组，先确认 GameSession::restore 成功且 CompanyOperations 完全相等，再仅从私有测试入口将该组 released_revenue 改为 MAX，直接传入内存 SaveSlot。
- 断言完整 GameSession::restore 返回 InvalidSave，并包含 `RESTORE-INS`、`RESTORE-GROUP` 与 GMM 诊断；损坏对象未经过 serde，因而真实覆盖 persistence 复核接线，而非只证明 JSON 入口拒绝。
- 从当前生产树显式重新准备：`flock /tmp/stock-market-gap-cargo.lock` 下执行 `cargo test -p engine --lib --no-run -j16 --target-dir .tmp/gap-target --message-format=json`，构建外部 deadline 为 300000ms。实际生产 crate 路径在 `.tmp/insurance-production-build.log` 中为本 worktree 的 `packages/engine`，准备耗时 33.61s，不把此耗时计作普通测试执行。
- Cargo JSON 给出的 production binary 为 `.tmp/gap-target/debug/deps/engine-6ae5d5d49d140673`，SHA-256 为 `4c135ab3344f5ec29de634175e7dd0f338e0b9d751dfd2691a77c52b361ec8c7`。未使用隔离 copy 的 binary；`.tmp/insurance-production-test-list.log` 明确包含原 22 case 的 math 极值和新增直接内存恢复 case，总数 23。
- `src/company/insurance` 全部 14 个 Rust 文件在编译前保存 SHA-256 清单、编译后逐项检查全为 OK；清单 `.tmp/insurance-production-src.sha256` 自身 SHA-256 为 `ab985aede63070840f99de4646afc0f987b3a06598b6057a40e1fc2fae5a65b7`，检查日志 `.tmp/insurance-production-source-check.log`。
- 普通执行：`timeout --kill-after=1s 10s <Cargo JSON binary> company::insurance --test-threads=8`，**23/23 通过，总 case 耗时 0.05s**；包括真实恢复 fixture，单 case 同样在总 10s deadline 内。结果日志 `.tmp/insurance-production-green.log`，binary 指纹 `.tmp/insurance-production-binary.sha256`。
- 原独立 reviewer 已审新增测试与唯一 cfg(test) 模块声明，确认没有弱化原 22 case、没有扩展交易规则，且最终生产树动态证据满足时限；最终核收见 [insurance-restore-review.md](insurance-restore-review.md)。没有遗留有效阻塞发现。
