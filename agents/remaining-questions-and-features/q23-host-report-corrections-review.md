# Q23 三 Rust 宿主报表更正接线独立复核

## 结论

2026-10-05，非实施者 `review_q23_hosts` 对本批 Q23 接线作静态独立复核：**PASS，无阻断 finding**。本复核没有修改 production 源码，没有运行 Cargo、测试、Git index 或 commit；绿色执行结果仍由 root 的统一构建与 binary 门禁登记，不能以本记录替代 runtime 验收。

## 范围与依据

- 阅读根目录 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、架构，以及 ADR-0010、ADR-0025、ADR-0030、ADR-0032。
- 核对 Server `actor.rs`、`routes.rs`、`lib.rs` 与新增 `tests/report_corrections.rs`；Desktop `actor.rs`、`actor/failure.rs`、`actor/protocol_tests.rs`、`lib.rs`、`lib_tests.rs`；WASM `lib.rs`、`protocol_tests.rs` 的 current diff 与调用上下文；连带查看 Native 原 fatal guards、Engine `company_corrections.rs`、`ProtocolSession` publication transaction 和日结 rollback。
- 对 shared files 的 Q08 指标、Q13 交割单、Q22 publication/ingress 前置 hunk 作上下文核对，不归为本项新增功能，不将仓库其他会计与 Web UI dirty files 宣称为本项完整验收。
- 大 A 语义依据沿用 `docs/company-accounting.md` 的 CAS 与 D8 游戏工程假设，以及 ADR 的现有 Money 分、股份股、T+1、日级持久化边界。本项不增加或改变交易制度，不新增会计公式；未重新访问官方网页，未把游戏日终控制入口或内存取消队列解释为现实法定更正申报流程。

## 三项门禁

1. **大 A 与跨层语义：通过。** 三宿主调用同一 Engine 更正队列；公开版本只在日终成功后安装，已完成请求不可取消，operation identity 绑定精确输入。pending 不写日级档，completed 在日级档恢复；保持既有工商账套支持边界，不把其他行业误装为通用更正。
2. **必要性与最小范围：通过。** 新增 submit/cancel/query 是应用控制接线，未接受任意 AccountId，未新增角色管理、数据库、WAL、schema version 或兼容分支。Server 使用现有 owner Bearer 验证与严格 DTO；Native 先校验当前 canonical generation，再在 actor 内取得当前 opaque epoch；WASM 使用当前 registry handle，已移除旧 handle 不得访问新局。
3. **错误与边界：通过。** 可恢复分支只识别 typed `SessionError::ReportCorrection`，不依据文本匹配内部错误；结构化 cause 保留。Engine 在局部 cloned operations/closing/library 全部成功前不安装更正，GameSession 日结失败回滚；ProtocolSession 日结 rollback 成功后才返回原 typed 错误，rollback 自身失败覆盖为非 typed 错误。Native 自动批次外层 publication transaction 丢弃未发布状态后暂停并通知，不关闭健康 actor/ingress；StepFatal、CorrectionInvariant、其他 Information 与 rollback/source publication 错误仍走 fatal 分支。state rollback 不重建或重新打开原 source，原 fatal 关闭断言没有因 Q23 被撤销。

## 测试证据边界

- 新增真实 REST 测试固定 owner、未知 account 字段、canonical/stale generation、精确重复、冲突、取消、完成事实及 restore；真实 Tauri invoke 固定注册与代际拒绝；WASM registry 固定旧 handle、pending 非存档与 typed 日结失败后的取消重试。
- Native 自动失败测试验证 recoverable notice、暂停与可继续取消；原 StepFatal guards 保留。Engine 已有输入拒绝与税不变量分类断言，未知内部错误保持保守 fatal 分类。
- WASM native registry 测试不覆盖真实浏览器的 JS plain-record 序列化执行；export 使用 `serialize_maps_as_objects(true)` 的静态契约与当前 record 对齐。此 runtime 边界已在实施记录明确，不能宣称完整三宿主矩阵或 Q23 整体核销。

未发现需要作者修复并再次复核的有效 finding。
