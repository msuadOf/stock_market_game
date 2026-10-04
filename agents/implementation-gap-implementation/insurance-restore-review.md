# G73 Insurance 恢复独立复核

## 范围与方法

- 复核者未参与实施；审阅 `insurance/groups.rs`、`groups/restore.rs`、`groups/restore_tests.rs`、`error.rs`、`behavior_tests.rs` 的完整改动与 `insurance-restore.md`。
- 对照原 `initial_measurement`、`preview_release`、`apply_release`、`preview_remeasure`、`apply_remeasure`、赔案入口以及 `docs/company-accounting.md` §2.4；未运行完整回归，未编辑产品代码。
- `persistence.rs` 接线由 root 负责，不在本次限定 diff 内；尚未据此宣称完整恢复完成。

## 大 A 语义与依据

- 改动不改变委托、撮合、T+1、结算、市场单位或证券类别规则；公司会计金额继续使用 `AccountingAmount` 的 i128 分，责任单元继续采用实际自然日。
- CAS 25（2020）§27/28、§29–34、§46–49 与 2026-01-01 其他企业适用窗口沿用正式文档已登记的财政部依据；本次没有新增精算规则，也没有重新联网核验法源。五项模型简化仍保留，不冒充完整 CAS 25 合规。
- GMM 调节式符号正确：初始亏损、即时重估亏损/财务增加 LRC，已释放财务回拨增加 LRC，服务收入减少 LRC；亏损成分为备查项，不重复加入组件余额。
- carry 采用有符号半偶舍入余数边界；保留终末失活 CSM 的历史非零 carry 与原算法一致。

## 必须修复的发现

- **P1：合法金额极值被恢复校验误拒绝。** `components` 先累计 `expected_claims_remaining + risk_adjustment_remaining + csm`、后减 `finance_remaining`，可在结果合法时发生中间 i128 溢出。例如 `premium=MAX`、`expected_claims=100000`、`RA=0`、400bp、365 天，原初始计量给出 `PV=96154`、`F=3846`、`CSM=MAX-96154`；组的 LRC 精确为 MAX，原建组路径可表示，但新校验先得到 `MAX+3846` 报错。右侧先加后减累计调节项也有相同问题。须采用精确可消去的多项和比较，不用静默 fallback 或缩小合法金额域，并增加合法极值 roundtrip 边界测试。

## 范围、测试与复杂度

- 独立 validator、自定义 Deserialize 调用及 InsuranceBooks 复核直接内存状态是 G73 必要范围；未扩展 Q17 强事务、历史来源证明或其他行业。
- 两个内部 apply 溢出测试仅更换损坏输入注入方式，原有全部结果及部分更新顺序断言保持不变；`cfg(test)` 注入不扩大生产可写面。
- 已覆盖损坏 released_revenue=MAX、覆盖期/进度、负余额、收讫越界、CSM/亏损互斥、越界余数、赔案金额与合法重估/终末失活 carry。
- 建议补充偶数责任单元的半偶舍入正负 tie（±units_total/2）合法接受测试，并对账套正常赔案支付后的 roundtrip 作代表性覆盖；属于边界强化，不应扩展历史证明。
- 首轮结论：存在上述必须修复项，当时尚未通过复核。

## 修复后二次复核

- 已重新审阅完整保险 diff；`signed_sum` 按项拆分正负 unsigned magnitude，先精确消去再合计剩余同号项，仅真正净额超 i128 才报错。`i128::MIN` 不直接取负，负结果通过 `-(magnitude-1)-1` 构造；零项已排除，因此负分支不会对零减一。P1 根因已修复，不增加依赖，也不缩小合法金额域。
- 已增加组内 MAX 一致状态与真实 `establish_group(MAX)` 的恢复测试；正负半单元 tie 经原 `preview_release` 生成后 roundtrip，覆盖建议中的偶数半偶舍入边界。
- 额外核对 root 的 `persistence.rs` 精确保险接线：`validate_company_domain` 的 company 循环调用 `InsuranceBooks::validate_restore`，将错误映射到带 company id 的 `InvalidSave`；位置先于未上市公司 `continue`，未漏掉未上市保险公司。其余跨主题改动不在本次复核范围。
- 建议后续直接固定新增求和 helper 的 MIN、真正正负净额溢出与右侧多累计项消去边界；目前未发现新的阻塞问题。
- 二次结论：限定改动通过静态独立复核；未发现新的大 A 语义漂移、范围扩张或不必要复杂度。定向 green 仍由实施者汇总，本复核未运行完整回归，不据静态结论冒称动态验收完成。实施记录的“待接线”状态应由 root 按实际接线更新。

## 最后增量复核

- 二次复核后仅新增 `cfg(test)` 的求和极值 case，产品 helper 未改变。已核对 MIN 自身、MIN+MAX+1 完全消去、MAX+1−1 可消去、MAX+1 与 MIN−1 真正净额溢出及 −MIN 拒绝的全部断言，符合 signed i128 值域与显式溢出契约，补齐前述建议。
- 实施记录已更新为恢复接线落地，并如实登记误共享 target、通报、停止与独立 target 修正；未用隔离 HEAD 产物冒充生产树 green。
- 该增量通过静态独立复核，未引入新的领域变化、范围扩张或阻塞项。隔离 green 与完整树定向 green 的执行状态仍由实施者和 root 汇总，不构成完整回归声明。

## 真实 SaveSlot 恢复增量复核

- 已完整审阅新增 `insurance/session_restore_tests.rs` 与 `mod.rs` 的 `cfg(test)` 模块声明；`books_mut` 是其他主题已有改动，不把它计入本次 G73 增量。
- case 通过真实 `GameSession::new` 生成公司前史，再完成休市自然日日结取得 SaveSlot；合法建组后恢复成功并断言 CompanyOperations 等值。之后只经既有私有 test 注入损坏 MAX released_revenue，要求 `GameSession::restore` 返回带 company、group 与 GMM 上下文的 `InvalidSave`，真实覆盖直接内存恢复接线，而非仅覆盖 JSON decode。
- fixture 固定 1 只沪市 MainBoard 股票、0 NPC、0 自动新组与 quiet shocks，使用 T+1 与真实休市日结边界；没有改变 A 股规则、伪造历史或用日内快照替代日终档。历史装配必要性来自真实 Session 恢复链验证，已消除无关行情与经营随机工作。
- 原 22 个保险测试未修改或删除断言，新增恢复链 case 只是补齐跨层边界；未增加生产可写面、依赖或不必要抽象。
- 静态增量复核通过；23 case 的实际执行时长与 10 秒外部 deadline 结果由实施者回传后登记，未先行把真实 Session fixture 宣称为满足运行硬限。

## 最终动态证据核收

- 已读取 `.tmp/insurance-production-build.log`、`.tmp/insurance-production-source-check.log`、`.tmp/insurance-production-test-list.log` 和 `.tmp/insurance-production-green.log`：构建路径为当前 production worktree 的 `packages/engine`，准备耗时 33.61 秒；14 个保险源码 fingerprint 全部 OK，test-list 为 23 case，包含求和极值与真实 Session 恢复新增 case。
- green 日志确认 23/23 通过，0 failed、0 ignored，总测试执行 0.05 秒；实际包含 `full_session_restore_rejects_corrupt_insurance_group_in_memory`，因此该新增 fixture 未达到 10 秒单 case 硬限。
- 实施者登记运行参数为 10 秒整进程 deadline、8 test threads；Cargo JSON 选择的 binary 为 `engine-6ae5d5d49d140673`，登记 SHA-256 为 `4c135ab3344f5ec29de634175e7dd0f338e0b9d751dfd2691a77c52b361ec8c7`。本复核核收既有日志，不另外重复执行测试，也不把 1095 filtered out 的定向验证描述为完整回归。
- 最终结论：本轮 G73 保险限定改动及真实内存恢复链独立复核通过，先前 P1 已修复并有真实 production 树定向 green；未遗留阻塞发现。
