# Native SQLite 与 Server/Desktop 日终存档独立复核

## 范围与依据

依据 `docs/principles.md`、ADR-0033、ADR-0025、Native SQLite/宿主源代码、`libsqlite3-sys 0.35.0` bundled SQLite、`windows-sys 0.61.2` 本地源码及 `.tmp/checklist-wave4/` 红绿日志。身份安全 routes 的完整审查由另一复核负责；本复核核对身份 schema 与市场控制表是否和经济存档分离。未运行 Cargo。

## 根因修复复核

- 成功日终 candidate：`ArchiveWriter::save_day_end` 只接受字段私有、无 `Clone`/`Deserialize` 的 `DayEndCandidate`。唯一构造入口 `capture` 调用真实 `ProtocolSession::save_candidate(key)` 并深校验。Server/Desktop actor 按 `CivilUpdate` 的 `seq_to` 与 settled date 捕获候选；日内普通 `SaveSlot` 无法传入经济写接口。copy 只复制已有 payload。测试覆盖成功日终后推进日内、普通 snapshot 不能替换已保存候选。
- 跨物理连接 writer 隔离：生产 `NativeDatabase::open` 先不截断打开真实文件并取得 OS 锁，再运行 SQLite bootstrap；锁与 DB clone 共享，最后一个 owner clone 释放。`from_connection` 私有，仅内存测试 factory 使用。测试覆盖同路径、hard-link alias、子进程冲突和 clone 生命周期。
- `renew_after` 与 Native writer metadata 的 owner 校验均在同一 generation map lock 下先检查 owner，再运行 `commit` 或访问 SQLite。非活动槽 delete 使用 private `delete_payload`，成功后才更新被删槽 generation，避免递归获取同一锁。`stale_writer_cannot_renew_or_commit_a_replacement_timeline` 与 `stale_writer_cannot_mutate_other_archive_metadata` 覆盖两类入口。
- Windows writer lock：Rust 1.96.1 std 源码 `File::try_lock` 在 Windows 以 `LockFileEx` 锁 offset 0 全文件范围，会干扰 SQLite 的 Windows byte-range locks。当前专用实现完整初始化 `OVERLAPPED`，在 `i64::MAX - 1` 锁一个 byte；此范围高于 bundled SQLite 最大文件范围 `(2^32 - 2) * 65536`，不交叠 `PENDING_BYTE`/`RESERVED_BYTE`/`SHARED_FIRST` 锁区。`windows-sys` 为 Native Windows target-only dependency。ABI/source 审查通过；bounds case Linux 绿不代表 Windows runtime 验收。

证据：`.tmp/checklist-wave4/native-p1-red-*` 和 `native-host22-*` 有首轮 candidate/物理 writer 根修的真实红绿；host23 CLI recovery 有 3 项精确绿。`native-lease-red.log` 为 renew guard 前真实红，`native-lease-owner-green.log` 为修复后 exact 1/1 绿；`native-metadata-owner-red.log` 为 metadata guard 前真实红。修复后 `native-host26-stale_writer_cannot_renew_or_commit_a_replacement_timeline.log`、`native-host26-stale_writer_cannot_mutate_other_archive_metadata.log` 分别 exact 1/1 绿；`.tmp/checklist-wave4/native-host26-archive-list.log` 汇总 16 项 Native archive 集成测试，`.tmp/checklist-wave4/native-host26-bounds.log` 为 Windows 锁字节范围 exact 1/1 绿。Desktop IPC 的修复前失败见 `native-tauri-metadata-red.log`，修复后真实 command case `native-host27-archive_metadata_ipc_rejects_stale_generation_and_replaced_session_owner.log` exact 1/1 绿；host27 另有三项 Desktop archive actor case 分别 exact 1/1 绿。Web Tauri mutation 契约亦有真实 red/green：`agents/day-end-storage/tauri-mutation-red.log` 与 `tauri-mutation-green.log`（25 passed）。这些是针对各自编译产物和 case 的证据，不代表完整 workspace 回归；本复核未运行 Cargo。

## 复核结论与边界

- Desktop 的 `tauri-host.ts` 对 `select/rename/copy/delete` 均传当前 generation；Rust rename/copy/delete 命令解析 generation 并派发至 actor 的 generation-guarded archive 操作。修复前后的 Tauri mutation 与 command 测试均有相应真实红绿证据，旧 metadata IPC 绕过 owner guard 的阻断已关闭。
- Native candidate、物理 single-writer lock、renew 与 metadata owner 检查，以及 Server/Desktop 的日终存档行为通过当前代码复核和所列短测试证据；没有发现新的阻断。
- Windows/macOS 的 writer lock runtime 尚未验收。Windows bounds case 在 Linux 运行只验证边界计算，不可代替 Windows runtime 验证；本复核未运行 Cargo，也未验证完整 workspace 回归。
- 身份安全 routes 的完整授权覆盖仍由另一 reviewer 负责，本结论不替代该审查。

## 其他核对

- Server `--database` 使用真实 SQLite 文件，Desktop 使用 app data 文件；bundled SQLite 与 Engine 解耦，生产初始化拒绝 `:memory:`。`--new-market` 与 `--archive-slot ID` 显式互斥；默认只恢复已选槽，缺失/坏槽不静默 fallback。选择落盘失败有独立错误，不假报恢复回滚。
- SQLite 接受当前精确 schema、拒绝未知 `user_version`/结构；无迁移、无 WAL，生产 journal mode 为 DELETE。A 股经济 payload 原样保存，Money（分）、股份（股）等单位不变；日期元数据用 ISO `CivilDate`，不按交易所开市日过滤休市自然日。
- Server/Desktop 在成功 `CivilUpdate` 后按 key 捕获并事务写入；写失败保留旧档并发出显式错误。活动槽删除清空 selection，writer 续建新 UUID，不暗选旧档。身份、凭据及 `market_control_grants` 独立于经济存档，删除经济档不删除身份或授权。
- Unix lock 源码核对为 `flock(LOCK_EX | LOCK_NB)`。Windows/macOS runtime 未验收。所有 Server 身份 routes 授权覆盖范围由另一 reviewer 审查。

## 结论

Native repository 在本次限定复核范围内通过：candidate、物理 single-writer lock、renew、metadata owner guard、Desktop archive IPC generation guard 及 Server/Desktop 日终存档行为均通过源码复核和对应精确短测试证据；没有发现当前阻断。该结论不代表完整 workspace 回归通过。本审查未运行 Cargo；Windows/macOS writer-lock runtime 尚未验收，Server 身份安全 routes 完整授权覆盖仍待另一 reviewer 独立审查。
