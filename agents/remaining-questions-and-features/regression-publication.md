# CompanyDisclosurePublished 回归修复

## 问题与依据

`company_event_contract::scheduled_publication_event_is_lossless_public_and_precedes_civil_advance`
等待含 `Report` 的日终事件批次后，仅按 `CompanyDisclosurePublished` 找到第一条披露，再将
该条当作 `Report` 解构。固定 seed 29 的真实经营路径在同批次先公布 `Announcement`，因此
失败于 `expected a public report publication event`，不是公开报告缺失。

`session/disclosures.rs::DisclosureDispatch::run_day_end` 和
`session.rs::record_civil_day_events` 明确采用 `Announcement`、`Report`、
`CivilDateAdvanced` 的顺序。公开资料类型与 immutable `PublicLibrary` 分离契约见
ADR-0016；定期报告与临时公告的区别见 `docs/company-accounting.md` 第 5 节。
修复不改变披露排期、交易制度、会计计算、市场经营或公开权限，也不把游戏内事件排序
声称为法定披露先后要求。

## 修复范围

修改范围仅为两个披露测试文件和本记录。`packages/engine/tests/company_event_contract.rs`
按 `CompanyDisclosureKind::Report` 精确定位待验证的报告。保留公开查询字段、版本、事件
payload 无私账内容以及唯一日历推进的全部原断言。

同一真实 fixture 额外要求首条披露为 `Announcement` 且先于 `Report`，通过事件 ID
查询 immutable `PublicLibrary` 并核对 company、published_at，验证整个事件批次 seq
严格递增，避免简单略过公告而漏掉相关跨类型边界。

`publications/weekend_publish.rs::weekend_report_publishes_without_trade` 的同类旧断言
只允许日历推进而排斥合法休市披露。原 binary 真实复现 seq 为 376、旧预期为 373。
修复保留零市场事件、day/tick/市场快照不变与周六排期报告的全部原检查，逐条验证只可
出现公开披露和最后唯一日历推进；每条 seq 恰好递增 1，各披露 ID 可在 immutable
`PublicLibrary` 查询并核对 company、published_at 和报告 revision。额外验证市场 RNG
和 NPC attention 不变，fixture 确实包含日终披露而非空事件弱覆盖。

## 验证

- 原 binary `.tmp/build-cache/full-regression/debug/deps/company_event_contract-e5faedc218f4a673`
  经外部 10000ms deadline、`--test-threads=4` 执行：4 项中 3 项通过、该项失败，1.80 秒。
- 修改后由 civil 修复 agent 按主 agent 协调统一编译；Cargo JSON artifact 为
  `.tmp/build-cache/full-regression/debug/deps/company_event_contract-0537d4f0b7b418c6`，
  features 为默认 `[]`；编译来源日志为
  `.tmp/main-regression-2026-10-05/civil-company-build.jsonl`。
- 新 artifact 经外部 10000ms deadline、`--test-threads=4` 执行：4 项全部通过，1.75 秒。
- 原 publications binary `publications-2ab027931077f61c` 经外部 10000ms deadline
  精确执行周六场景：真实失败于单一日历事件的旧 seq 预期，1.23 秒。
- 修改后主 agent 统一 Cargo JSON 确认新 artifact `publications-b77ff28a117d5f97`。
  外部 10000ms deadline、`--test-threads=16`、`RAYON_NUM_THREADS=4` 执行整个
  publications 套件：22 项全部通过，2.09 秒。
- 本记录不替代主 agent 的完整回归与非作者独立复核。
