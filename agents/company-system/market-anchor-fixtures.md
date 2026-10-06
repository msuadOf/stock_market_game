# MarketSnap 新增字段测试 fixture 修复

## 范围

首轮仅修复 `.tmp/company-system/session-actions/web-tsc-bounded-build.log` 指出的九个 `*.test.ts` 文件。后续短检查发现共享 `protocol-test-fixtures.ts` 的 `market()` 也缺少同样字段，因此一并补齐。没有改生产代码、generated 类型、strict schema、JSON 或测试断言。

## 字段取值依据

- `cash_ex_reference_pending_trade: false`：这些 fixture 都没有设置“除息参考价已安装、等待当日首笔成交”的状态。
- `last_cash_ex_reference: null`：fixture 没有提供历史除息参考价；没有虚构除息事实。
- `day_market_activity` 按 fixture 表示的市场状态填写：证券排序、盘口行同步、盘口初始快照、协议本地刷新和快捷交易 fixture 没有表示当日成交，设为 `false`。SSR 金额 fixture 含有逐笔成交，移动行情 SSR fixture 含当日成交量或逐笔成交，设为 `true`。

三个字段只添加到日志涉及的测试 fixture；继承原 fixture 的对象继续自然保留这些字段。

## 验证

九个目标测试文件的短测由 root 执行，63/63 通过，日志见 `.tmp/company-system/session-actions/market-anchor-fixtures-short-web-cwd.log`。app 与 node TypeScript 全量检查通过，日志见 `.tmp/company-system/session-actions/web-tsc-review-final.log`。

共享协议 fixture 补齐后，protocol-parse 与 protocol-runtime-store 定向短测由 root 复核，14/14 通过，日志见 `.tmp/company-system/session-actions/protocol-anchor-verified.log`。两批变更均获非作者 Luna 独立复核通过：九文件复核由 `market_anchor_fixture_luna_review` 回报；共享协议 fixture 复核由 `review_protocol_market_fixture` 回报，并核对了 `.tmp/company-system/session-actions/protocol-anchor-final.log`。两份复核结论通过审查消息回报 root，没有独立落盘审查记录。

以上仅覆盖指定测试文件、共享协议 fixture 和 app/node 类型检查，不代表完整 Web 或项目回归通过。
