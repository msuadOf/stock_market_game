# 重集成批次1（web 可直接取回三组）实现与复核归档

> 2026-10-19。分支 `feat/web-reintegration-slice1`（基于 main dfd55bc8）。
> 实现：重集成首片实现 agent；复核：独立 fork（未实施本批改动），只读。

## 取回结果（11 文件 / 621 行，纯新增零既有改动）

| 组 | 提交 | 文件 | 自身测试 |
|---|---|---|---|
| G5 成交确认表 | 751763e0 | TradeConfirmationTable.tsx + trade-confirmation-table.test.ts（2） | 2/2 绿 |
| G3 均线组件半 | 3094f08f | MovingAverageSettings.tsx、moving-average.ts(+test)、config/moving-average-settings.ts(+test)（5） | 5/5 绿 |
| G7 存档管理 | 921b2dfa | ArchiveManager.tsx、save/archive-store.ts(+test)、save/indexeddb-save-repository.ts（4） | 3/3 绿 |

取回锚点均为 `git checkout 18725d07 -- <pathspec>`（盘点 §3 批次1 原样执行）。

## 验证口径

- `tsc -b`：取回前后错误集逐行 diff **完全一致**（main 既有 32 错，11 个新文件零报错）。
- 全量 `npm test`：唯一稳定失败 `save/file-target-permissions.test.ts` 与 wasm worker
  两个 flaky 均**先于取回存在于 main 基线**（纯 main 单独复现），取回文件相关失败为零。
- oxlint：11 个新文件零告警匹配；既有文件 warning 为基线状态。

## 复核结论（阻断 0 / 建议 2，已登记）

1. **旧 main 存档格式兼容提示（批次2/3 接线时须知）**：取回存档栈按 HEAD 新格式校验，
   旧 main 格式存档会被 `schema/root.ts:51-55` **显式拒绝**（正确非静默行为）。
   接线 ArchiveManager 时若需读旧档，须先做格式迁移，不能放宽校验。
2. **任务书"基线 99/99"口径过时**：main 实测分片合计约 663 tests（含 1 个既有稳定失败
   file-target-permissions）。后续批次验收应以"取回文件相关失败=0 + 既有失败集不扩大"
   为口径，勿沿用 99/99。
3. 双栈并存（取回 archive-store 栈 vs HEAD 在树 save-repository 栈）已在 921b2dfa
   提交信息登记待裁决，复核确认非阻塞。

## 勘误（对盘点文档）

- `utils/turnover.ts` "影子退役未入 manifest" 标注**过时**：retired-files.txt 第 282 行
  自 56268e5f（合并提交）起已有登记（git log -S 唯一命中）。无遗漏需补，本片不取回
  （消费者 calendar-candles/分钟K线/host 层均在批次2/3）。
- 盘点 G7 计 "5 文件(+2 test)" 勘误：18725d07 无 indexeddb-save-repository.test.ts，
  实取 4 文件。

## ADR 抽查证实

ADR-0042（maker/taker 腿方向）：11 文件零 maker/taker 引用；
TradeConfirmationTable 的 `side` 为成交记录自身字段，非腿方向推导；三项费用
（佣金/印花税/过户费）与 `docs/trading-rules.md:101` 口径一致，测试 Buy 腿
stamp_tax="0" 符合"印花税恒卖腿"（:44）。ADR-0041（Coded\* 词表）：零接触。
