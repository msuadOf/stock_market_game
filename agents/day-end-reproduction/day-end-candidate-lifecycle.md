# Server 日终候选生命周期排查

- 日期：2026-11-09。
- 本记录是待修缺陷的复现证据，不代表问题已修复。
- 未改交易、公司经营、结算、T+1、金额或日期语义。周六/周日两个 Closed 日仍是真实成功 CivilUpdate，而非伪造 tick。

## 已执行的最短复现

探针源文件为同目录 `day-end-capture-repro.rs`。它引用仓库现有 fixture；运行前复制到 `packages/native-store/tests/day_end_capture_repro.rs`，结束后移除该临时测试入口。

```sh
cp agents/day-end-reproduction/day-end-capture-repro.rs packages/native-store/tests/day_end_capture_repro.rs
PATH=/Users/asuad/.cargo/bin:$PATH node scripts/run-long-validation.mjs 300000 -- /Users/asuad/.cargo/bin/cargo test -p native-store --test day_end_capture_repro --no-run -j 10
node scripts/run-with-deadline.mjs 10000 -- target/debug/deps/day_end_capture_repro-f879fec27697c5a2 --test-threads=3 --nocapture
rm packages/native-store/tests/day_end_capture_repro.rs
```

二进制后缀与具体构建相关，应使用 `--no-run` 输出的路径。编译明确使用 `-j 10`，首次 35.63 秒、增补第二个边界后 2.05 秒；测试以 3 个线程并发，外部进程树上限 10000ms，测试本身 0.42 秒。

结果：1 passed，2 failed，均因预期的行为断言失败，不是编译或 fixture 失败。

1. 对照 `without_automatic_capture_an_earlier_published_candidate_is_available` 通过：两个成功日结后，不经过自动捕获的首日候选仍可精确读取。
2. `server_archive_capture_preserves_an_earlier_published_candidate_for_the_client` 失败：首日 `DayEndCandidate::capture + ArchiveWriter::save_day_end` 后，当日重复读取可以成功；次日成功写入后，再读首日失败为 `InvalidSave(ValidationFault { code: 3016, context: [20300105, 4] })`。真实 SQLite 最新槽已明确验证为次日。
3. `another_clients_baseline_preserves_an_earlier_published_candidate` 失败：两个日结后执行另一客户端获取 baseline 所调用的 `prepare_public_baseline()`，首日 key 同样失败为 3016。

## 代码链与资源边界

- `apps/server/src/actor.rs::persist_day_end` 在广播成功 CivilUpdate 前调用 `DayEndCandidate::capture`。
- `packages/native-store/src/lib.rs::DayEndCandidate::capture` 调用 `ProtocolSession::save_candidate`。
- `packages/engine/src/session/protocol/civil/session.rs::save_candidate` 返回完整克隆后，从 `pending_save_candidates` 删除该 seq；最新 `day_end_save` 临时允许重复读，下一自然日就不再能读旧档。
- 任意客户端获取 public baseline 还会全局清空 pending；因此只改自动捕获不删除仍不能实现多客户端正确性。
- WS 广播容量为 1024，之后每客户端 Publisher 还有独立的 65536 units 缓冲。Pull 客户端可延迟 GetFrame；现有协议没有日终候选 lease/ACK。用固定候选数量冒充既有交付契约不可靠。
- 当前 SQLite 主库只更新一个槽，不保存可供精确回看所有日期的历史版本。新增永久历史表会改变持久化范围；连接级全历史 TEMP 表仍可能每天积累数百 MB，占满磁盘，不作为修复。

## 可实现的后续方向

显式文件目标先注册当前 WS、generation 的导出订阅；只有存在订阅时才生成精确候选 spool。多个订阅共享同一份 payload，各自消费后 ACK，所有 owner 释放后删除。取消目标、断连、换代均释放 owner。每个 session 必须有 spool 字节预算；超过预算时显式停止对应文件导出并提示，不能静默丢日或暂停整个共享市场。普通自动 SQLite 保存不参与此队列，也不改变现有主库 schema。

该方向需要 EngineHost/RemoteHost、文件目标生命周期、WS 协议和 actor 一致实现，不能只凭存储模块测试宣称远程文件多日导出端到端完成。

## 已完成的 native-store 模块

- `DayEndExportSpool::new(byte_limit)` 显式注入 payload 总字节额度，无产品默认值；不改自动存档槽和主数据库 schema，也没有增加依赖。
- 独立 SQLite 临时数据库使用空文件名创建，连接关闭自动删除；显式设置 `temp_store=FILE`、`cache_spill=ON`、`cache_size=-2048`、`mmap_size=0`、`auto_vacuum=FULL`，数据库页上限在 payload 额度之外保留 4 MiB。
- 这里的额度是共享候选 payload 总字节，不是包含 journal、元数据和页对齐开销的精确总磁盘额度；2 MiB cache 是 SQLite 缓存目标，不是整个导出流程内存硬上限。
- `subscribe` 注册独立 owner；`publish` 只为当时存在的订阅保留精确日期/seq 候选，多订阅共享同一 payload；`read` 可重复读取且不消费；`acknowledge` 只释放当前 token 对该精确候选的 owner；最后一个 owner 离开后删除候选。
- `unsubscribe` 可重复清理，删除无 owner payload；`reset` 一次清空订阅和候选，供换代接线使用。
- 无订阅时 `publish` 不序列化、不保留历史。容量不足保持已有 pending 数据不变，返回带字节上下文的 `StoreError::ExportCapacity`，由 actor 显式处理慢订阅。
- 发布前使用不存 payload 的计数 writer 统计实际 UTF-8 JSON 字节，先检查既有默认 512 MiB 单档解码上限与剩余总额度，通过后才分配精确长度的 payload；超过读取上限使用 `ExportCandidateTooLarge` 明确拒绝，不保留永远读不了的大档。

## TDD 与独立复核

- 先建立四项 integration 行为测试与可编译空实现；2026-11-09 红灯为四项预期断言失败，用时 0.23 秒。随后实现后四项全绿。
- 测试覆盖精确两日、重复读取、两个独立客户端 ACK、共享 payload、晚订阅无追溯授权、无订阅无保留、reset、容量失败保旧数据与释放后恢复、错误日期/未知 token/重复发布拒绝，以及 `subscription_tokens`/`pending_bytes` 在部分释放后的统计。
- 增补两项 unit 验证 UTF-8 字节计数的精确上限与显式 SQLite 资源参数。
- 初次独立复核发现“先完整分配再检查容量”和“写入候选可超过默认读取上限”两项有效问题；均修复。第二次独立复核未发现模块新的必须修复问题，独立并行运行 4 项 integration 与 2 项 unit 全绿，用时 0.62 秒与 0.00 秒。
- 各测试命令使用 10000ms 进程树 supervisor 和 4/2 个测试线程；编译以 `-j 10` 和共享 300000ms 上限执行。没有执行完整回归。
- 该独立复核仅覆盖 native-store 模块、探针及本记录。Server actor/routes 和 Web 文件订阅接线仍须在完成后进行完整跨层 diff 复核。
