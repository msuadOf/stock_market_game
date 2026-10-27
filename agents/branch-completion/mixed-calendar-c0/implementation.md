# mixed Calendar C0 实施记录

- 日期：2026-10-25；分支：`fix/engine-mixed-calendar-c0`。
- 依赖：#116 源提交 `6631905bce83d2701c129c3dfc5130a2a1ac8a78`；其后 `2c708ddf` 仅加证据。整合时只取本 C0 commit，不重复 #116。
- 冻结设计：`agents/branch-completion/mixed-calendar-design-2026-10-25.md` §5，SHA-256 `f8eb6323602935c0c6d205a2c52f0bccd8c39a5ef5b9fdb3e2b8d175911a4711`；独立设计批准见同目录 `mixed-calendar-c0-design-review-2026-10-25.md`。

## 语义与范围

`CivilClock` 从 setup 实际全部交易所建立 canonical 非空集合；`phase`、共享会话 union 计数和 `next_status` 共用同一个 OR 聚合。全闭市原因只取 canonical 第一所代表诊断，逐所原因仍查询冻结日历。原单所 API 委托集合 API；存档不加字段。恢复 current 越冻结 policy 上界在返回前以 `CivilClockError::Calendar` 拒绝，避免随后 `phase` panic。公司 due 仍只检查日期适用范围。

依据为 2026-07-06 生效的上交所《交易规则（2026年修订）》2.4.1、深交所同规则 2.3.1：休市由本所公告界定；2026-10-25 原件与摘要已由设计归档。已读取归档正文对应条文，合成测试来源一律为 `synthetic-test`，不冒称真实年度通知。

仅修改 clock、GameSession 两处装配、`civil_clock` 测试及最小两份正式文档。不接逐股行情/撮合、策略、本股时钟、恢复历史校验或并发收件；C0 不构成 mixed 完整可玩证明。

## 红 → 绿

先在 #116 基线追加使用现有 API 的合法双股票 GameSession 恢复 fixture：2030-01-02 开局，SSE 合成 Official 休市覆盖 01-01..02，SZSE 仍开市。严格 restore 先成功；两所开闭市独立断言成功后，`phase` 实际 `ClosedDay`，预期 `IntradayTrading`。目标失败证据见 [red-mixed.log](red-mixed.log)，不是缺 API 编译失败。然后才实施集合聚合。

最终同 fixture 覆盖两种首股顺序、两种休市所、冻结完整 policy；纯 clock 短矩阵覆盖空集、顺序/重复、全闭市、union 非累加、未配置所不参与、次日聚合、异原因 canonical 代表、自然日 due、保存恢复、单所包装、闰日与上界、digest 损坏和 current 越界 typed 拒绝。保留原重复/跳日、observer 与上界断言。

`civil_clock` 二进制 19 case 全绿，4 个独立 case 进程并发；每 case 与整批外部进程树 deadline 均为 10000ms。最慢有效 case 为完整 GameSession 四组合恢复 2947ms。原有 `closed_days_accrue_without_trading` 与 `closed_day_fixture_facts_probe_is_deterministic_evidence` 在 native runtime 下既有早退，未将这两个早退当作新增行为证据。详细结果见 [green-results.json](green-results.json)。未跑全量回归。

build-only 使用外部 `runBoundedCommand` 300000ms、`cargo -j8`、`CARGO_PROFILE_DEV_DEBUG=0`、`CARGO_PROFILE_TEST_DEBUG=0`，目标隔离在根 `.tmp/mixed-calendar-c0/target`。观测到并行 cc1 与 rustc；首次误用默认 debuginfo=2 后主动终止，改用与 #116 相同 debug=0。初次 API 矩阵 build 的 HolidayKind 拼写错误已修正；不计作行为红测。最终绑定 build 0.18s，全部依赖 fresh，测试随后重跑且源文件清单前后不变。

## 绑定与复核

[binding.json](binding.json) 记录完整产品文件清单 hash（2166 文件，排除 agents/memory 工作记录）、最终二进制 SHA-256、真实 Cargo 产物日志 hash、五个产品改动文件 hash。完整清单与原始构建/逐 case 日志位于根 `.tmp/mixed-calendar-c0/`。`git diff --check` 通过。

实现后交未参与实施的 `sol61_tree_review` 审完整 diff；批准前本记录只表示候选已实现与短测通过，不宣称完成。
