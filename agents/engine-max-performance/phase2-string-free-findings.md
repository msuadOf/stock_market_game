# 阶段二无字符串化 findings（2026-10-07）

## 改动清单

| 提交 | 内容 |
|---|---|
| 8447ecd6 | 错误结构化：321 处 ValidationFault + money/orderbook 枚举载荷 |
| eef4f262 | 复核修复：escrow 双股动态 id + 目录防线 |
| 9bf4fe9b | venue/frozen 枚举化（OrderVenue/FrozenAsset） |
| 2b50c6b2 | 三宿主错误 fault 码化（1290-1314） |
| 4f44063c | web 前端数值 id 迁移（摄入层翻译 + IntradayPanel bug 修复） |
| 95e70407 | 公开报告游标错误数值化 |
| 95fcc02e / 6fa75cd0 | docs |

## 测试

- engine lib 1314 过/8 败 = 基线完全一致（8 个均为 dde740c5 已有）
- 全部集成二进制与基线逐一对齐（基线 worktree 对照）
- server/wasm/desktop 全绿（仅基线固有失败）
- web npm test 0 失败 + tsc 0 错误

## 基准

阶段二合并（H=阶段一末 254c50b0，C=95e70407）：锚漂 -2.91%；mean +1.89%，
区间重叠，非全部 >1。**判定：性能中性**。性能收益来自阶段一 StockId（+22.73%）；
阶段二是契约改进，无回退。

## web 修复的真 bug

IntradayPanel 漏传 activeDailyCandle 给 MobileIntradayProjection——日内均价
恒显示"不支持（缺少真实成交额统计）"。这是 HEAD 既有 bug（非本批引入），
数值迁移暴露后修复。

## 外来文件处置

`.tmp/e8/foreign-web*/` + `.tmp/e8/foreign-tests/`：main 分支领先的未跟踪
文件，引用本分支不存在的 API（u128 周转/AccountId 字符串契约/PublicTrade/
税务 schema/report-correction/archive 等）。全部隔离未提交、未删除。
建议：main 合并时自然消解；如需提前可从 main cherry-pick 对应生产代码。

## 未竟（需独立 session）

- **CompanyId 数值化**：209 处非测试引用，设计 spec 在进度档。规模与 StockId
  相当，需专注执行。
- **`&'static str` 标签清理**：calendar/experience/config/observation 等的
  字段名标签（~215 处）——多数在错误枚举已随 ValidationFault 消解，残余
  在 Domain enum 的 Display 路径。
- **清单 D 序列化投影**：company/query.rs 金额字符串投影的来源字段核对。
