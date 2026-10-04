# Q17 报表更正事务原子性

## 范围与 API

- `ClosingEngine::correct` 覆盖调整凭证过账、重述底稿登记与派生 `ReportSet` 生成；报表失败时保留调用前的 `Books` 与 `ClosingEngine` 状态。
- `PublicLibrary::correct_and_publish` 是完整公开更正 API，额外预检更正关系、批准/公布时点、公开报表形状及内容摘要，再提交 `ClosingEngine` 版本与 `PublicLibrary` 条目。
- 当前没有 Session/UI 公开更正调用方；本项不造占位入口。
- 失败错误保留用户提供的更正理由和底层失败层。失败不写业务凭证、不写公开版本、不伪造现金或业务失败记录；相同凭证可在修正其他输入后重试。

## 领域边界与来源

- 原公开版本与已登记期间版本不可覆写，更正是新版本并链接 `supersedes`。
- 已登记 D2 继续适用：损益按重述目标期间，现金流按实际收付期间列报，并以显式重述现金调整行配平，不做 plug；重述映射持续用于后续期间生成，禁止把更正损益重复计入后续当期损益。
- `docs/company-accounting.md` §2.7 登记 CAS 28 更正/重述为当前游戏假设。来源台账 `packages/engine/tests/fixtures/company-model/policy-sources.json` 的 `cas-28-changes-errors-2006` 仍标记 blocked：CAS 28 原文未取得，仅由财会〔2026〕11号第六十三条核验准则存在性。因此本实现只落实既有游戏语义与事务完整性，不声明 CAS 28 合规，也不新增会计判断。
- 更正的间接法双口径和历史不可变语义由 CAS 30（2026）财会〔2026〕11号第二条、第三条、第六十三条及既有 D2 口径约束；适用日期按 `docs/company-accounting.md` §4 已登记范围。本次不改变会计列报。

## 实现与验证

- `Books::transact_post_batch` 暂存试算 `Ledger`，以 Journal 批次数 checkpoint 在闭包失败时回退本次追加批次与来源索引；不复制完整 Journal 或历史 `Books`。
- 报表生成与公开库插入都先形成候选；公开内容摘要计算、形状检查和时序检查成功后才发生状态提交。事务外不暴露中间态。
- TDD 回归覆盖派生行业报表错误及最大合法期初余额上的报表合计溢出，两者均断言账套/结账登记簿序列化状态不变；组合 API 覆盖错误发布时点回滚三层状态、保留理由，以及同一凭证重试成功。既有 `industry_reports/correction_restatement.rs` 覆盖跨月与跨年重述行为及现金只计一次。
- root 使用 `--jobs 32` 统一编译通过，编译与短测分开执行。五个确切用例分别受外部 10000ms deadline 约束并并行执行：行业报表生成失败、报表加法溢出、后续期间不泄漏、既有更正守卫及公开更正失败／重试，全部通过；每项实际执行为 0.00–0.01 秒。日志位于 `.tmp/checklist-wave2/`，未运行完整回归。
- 非作者 `review_q17_atomic` 已审查完整 diff，并对精确失败原因断言修正再次复核通过；未以编译错误替代业务失败测试证据。
