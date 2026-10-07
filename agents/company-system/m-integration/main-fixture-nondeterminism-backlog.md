# 主档 fixture 运行间非确定性（backlog，独立排查主题）

> 登记时间：2026-10-07（M 批集成复核 note 级发现）。
> 本主题**不在当批判次内修复**，仅登记现象、复现方式与疑似方向，作为独立排查主题。

## 现象

主存档 fixture producer（`../main-save-fixture-generator.rs`，五股显式 setup、
seed 42、2030-01-07 开局并完成两个完整交易日日结）在**同一 seed 下两次独立运行**
产出的 `current-schema-save.json` 的**行情切片（OHLC / K 线与成交历史）不一致**；
对照同批的 `closed-day` 与 `minimal` 两个 fixture producer 可以**字节级复现**。

差异面观察（来自 M 批集成期两次运行的对照）：

- `company_system` / 账户 / 公司行为切片一致（简单整数事实，不涉及行情路径）；
- 差异集中在依赖**撮合与行情演化**的切片（OHLC、成交历史等随价格路径变化的数据）。

## 复现方式

1. `cargo build --release -p engine`（release Engine rlib 为 producer 的直连依赖）；
2. rustc 直连 rlib 编译 `../main-save-fixture-generator.rs` 为独立 producer
   （同 M 批做法，可参考 `../../../.tmp/company-system/m-integration/gen-main-build.log`
   的编译记录）；
3. 以同一 seed 连续运行两次，分别输出到不同路径：
   `node scripts/run-long-validation.mjs 300000 -- <producer> out-a.json`、
   再跑一次得 `out-b.json`；
4. 对比两次输出：`company_system` 等切片相等，行情切片不等；
   对照 `closed-day-save-fixture-generator.rs` 与
   `minimal-save-fixture-generator.rs` 同法两次运行字节相等。

## 疑似方向（未验证的假设，排查时逐条证实或排除）

- **并行撮合顺序**：主档场景启用了多 NPC / 多标的的并行执行路径（closed-day /
  minimal 场景更小或休市，未覆盖该路径）；若撮合或事件派发按线程完成序而非
  确定性序落账，行情切片会随运行抖动。
- **容器迭代序**：依赖 `HashMap` 等无序容器的遍历序参与撮合/订单簿推进时，
  同 seed 也会因每次运行的迭代序差异产生不同成交序列（对照：engine 性能主线
  曾做过 string-free / 无锁化改造，需核对其是否引入非确定性迭代）。
- **多线程 RNG 分流**：若每个 worker 线程独立持有从同一 seed 派生的 RNG 且
  消费顺序随调度变化，行情抽样会不可复现。

## 排查登记要求

- 修复须以「同 seed 两次运行字节相等」为主档 fixture 的验收断言收口（producer
  内置守卫或独立校验脚本均可），并同步核对既有三份 fixture 与 `current-company-slice.json`
  的再生成口径；
- 结论回填本文件并同步 `../rights-and-repurchase.md` / `../simple-preferences.md`
  相应 fixture 章节，避免再次出现台账与实际可复现性不符的失实记录。
