# 日 K 公历表示上界独立复核

## 范围与结论

- 复核日期：2026-10-05。复核者未参与实现；完整审查本批 `apps/web/src/utils/candle-date.ts` 与 `apps/web/src/utils/candle-date.test.ts` 的 diff，并补审 `packages/engine/tests/calendar/future_representation.rs` 全文、`packages/engine/tests/calendar/main.rs` 的模块注册 diff、`agents/remaining-questions-and-features/calendar-future-range.md` 全文。
- 结论：限定 PASS，无 P1/P2 发现。仅确认日 K 表示、host 解析及 save 解析允许 1998–2199 年 UTC 零点 Unix 秒标签；不确认默认市场可运行到 2100 年，也不确认任何未来官方休市安排。
- 已阅读根目录 `AGENTS.md`、`docs/principles.md`，并核对相关 ADR-0018 的 11.2.7 节、`docs/open-questions.md` 中日历关联内容。

## 大 A 语义与可靠依据

- `packages/engine/src/calendar/date.rs` 的既有 `CivilDate` 算法验证窗为 1900–2199。公历日期表示不是交易日历许可；本批没有改动交易规则，因此不存在新增未来交易所规则依据的声明。
- `CalendarPolicy::current_default_policy` 的 `runtime_max_end` 仍为 2099-12-31，内嵌 `LunarYearFacts` 覆盖仍为 1998–2099；不能用此次表示层放宽代替事实表或政策扩展。
- `CalendarPolicy::check_bounds` 检查边界顺序及默认开局位置，没有把自定义政策硬锁在 2099 年；`check_facts_coverage` 要求初始化下界至运行上界逐年具备事实。`LunarYearFacts::check_shape` 以 `CivilDate`、同年及有序性校验，不额外锁死 2099。因此提供完整、通过校验的事实表及政策后，2100 年可由既有结构合法表达，原 Web 上界会独立拒绝其日 K 标签。
- 新错误消息明确使用“公历可表示范围”，不再把表示边界称为交易日历范围。UTC 零点、Unix 秒、下界 1998 和两层错误定位语义均保持不变。

## 必要性、边界与复杂度

- 改动集中于一个共享 helper 的上界及消息、一个测试文件；未增加 schema、兼容路径、依赖、未来假日猜测或默认政策变更，符合最小范围。
- 新用例穿过 host 与 save 的真实日 K 解析入口，覆盖 2100 年进入新范围、2100-02-28 的非闰世纪日期及 2199-12-31 最后一天；2200-01-01 在两层仍明确拒绝。既有旧相对日期、非零点及 1998 下界检查保留。
- 新用例仅测试日期标签，不将 2100-02-28 是否为交易日作为前提；不会把周末、节日或未来制度许可混入纯传输校验。测试不证明 engine 自定义政策运行闭环，此限制不构成本批表示层修复的遗漏。
- 未发现本批新增的跨层语义漂移或不必要复杂度。仅凭最终 diff 无法独立证明 TDD 红阶段顺序，本复核不对此作保证。

## 补充 engine 测试与工作记录复核

- 新测试只从既有默认政策复制事实后附加测试合成的 2100 年一行，重算 `LunarYearFacts` 与 `SimulatedFallbackRuleset` digest，将自定义上界显式设为 2100-12-31。没有生产日历变更或官方覆盖条目，测试名称明确排除官方声明，工作记录明确声明日期为合成。
- `SimulatedFuture` 断言符合现有 `year_label` 以通知年份 2026 分类的逻辑。2100-01-01 为星期五，现有模拟规则将元旦标为 `SimulatedHoliday(NewYearDay)`；2100-01-04 为星期一且不在该合成事实的假日范围，断言 `Trading` 合理。星期值另以 Node UTC 日期计算核对。
- 从 2100-01-04 向前查询 360 个交易日，必然早于 2099-01-01：两日期之间最多约 263 个平日，额外休市只会继续向前推进。现有前史算法逐日递减，没有新的跳年假设。政策 JSON 恢复后通过 `TradingCalendar::from_policy` 复核 digest 并与原日历比较，也符合既有恢复机制。
- `main.rs` 仅注册新测试模块；补充测试为表示层修复提供自定义政策机制证据，没有借此扩展默认游戏运行范围。工作记录明确保留未来规则选择及末年报告排期问题，不误报领域能力完成。
- 补审仍为限定 PASS，无 P1/P2 发现。Rust 用例由 root 编译并实际执行，本复核者未独立重跑；随后亲读 `.tmp/checklist-wave4/calendar-future-representation-green.log`，确认 exact 用例 `future_representation::explicit_test_calendar_can_represent_and_restore_2100_without_official_claims` 为 1 通过、0 失败、11 filtered out，耗时 0.01 秒。
- 已亲读工作记录更新后的证据段，其 root 真实执行结果与上述日志一致，并明确限定为自定义合成政策机制验证，不将编译替代执行，也不扩大为默认未来日历已实现的声明。编译耗时、artifact 定位及 deadline 参数由 root 记录，复核者此次核验的是实际执行日志与结论一致性。

## 独立短测

两项命令并发执行，各自使用进程外 10000ms deadline、Node 10000ms case timeout 和 `--test-concurrency=8`。未执行 Cargo、完整回归或 Git 提交。

```sh
node scripts/run-with-deadline.mjs 10000 -- node --experimental-strip-types --test --test-timeout=10000 --test-concurrency=8 apps/web/src/utils/candle-date.test.ts
node scripts/run-with-deadline.mjs 10000 -- node --experimental-strip-types --test --test-timeout=10000 --test-concurrency=8 apps/web/src/host/protocol-civil.test.ts apps/web/src/save/save-schema-contract.test.ts
```

- 第一项：2/2 通过，0 失败，退出码 0，Node 报告约 246ms。
- 第二项：36/36 通过，0 失败，退出码 0，Node 报告约 322ms。
