# 五日／日期历史 UI 独立复核

- 复核日期：2026-10-05
- 复核范围：`RetainedHistoryPanel.tsx`、对应 CSS、`retained-history-model.ts` 及两项测试；`MarketRuntimeProvider` 的 `queryMarketHistory` 接入；`LocalRefreshViews` 的五日和日期查询插入；`MobileStockDetail` 的历史内容位及五日启用。
- 排除范围：共享文件内与本批无关的既有改动；生成绑定由 root 生成，本复核没有手写或评估生成产物正确性。

## 结论

在上述限定范围内，未发现需要阻止合并的 UI 语义或范围问题。实现符合 ADR-0034 与 core contract：五日是查询窗口；仅显示已结束自然日的分钟事实；最近五个有效日按本证券 `Traded`／`NoTrades` 选择，`Closed` 不被算作开市日；当前日、`BeforeStart` 虚拟日K及休市日期不会变成分钟事实；零成交与休市状态有不同文案。分钟金额和股数按 wire 十进制字符串显示，成交阶段、OHLCV、成交额及笔数均保留契约单位。图表使用真实分钟 bar 的收盘价点，不插值补齐空白分钟。

组件的范围切换会使旧请求失效，Provider 也会在宿主、generation 或玩家账户变化后拒绝迟到响应；日期分页沿用排他日期游标并缓存已取页。失败会显示查询错误及详情，没有静默吞错。新增组件独立留在 Web UI 层，未将领域计算放入 Engine。

## 验证

执行短测：`node --test --test-timeout=10000 src/components/retained-history-model.test.ts src/components/retained-history-panel.test.ts`（工作目录 `apps/web`）。6 项通过，0 项失败。没有运行 Cargo、构建或复杂回归。

## 限制与依赖

本结论只复核 UI 消费契约和指定接入点。按父任务说明，host wire owner 仍在实现依赖；本复核不将该宿主接线、IndexedDB／SQLite 持久化或 generated binding 宣称为已验收。应待 owner 完成后由 root 核实端到端可用性。
