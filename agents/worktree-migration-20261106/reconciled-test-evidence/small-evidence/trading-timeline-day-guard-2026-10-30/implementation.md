# TradingTimeline 跨日开盘窗口守卫

日期：2026-10-30
基线：`0ae6153fc3749744a9465a7c99ac30b6ad8f43af`
分支：`codex/trading-timeline-day-guard-20261030`

## 变更

`currentMinuteSlot` 保留原 `tick > openingTicks` 前置条件，因此 tick 0 及原先未进入 `minuteSlot` 的输入仍返回 `null`；其余输入先经 `eventOffset` 计算日内偏移，只有日内坐标已越过开盘窗口时才调用 `minuteSlot`。`minuteSlot` 本身、事件 tick 校验、交易阶段和事件时间换算均未改动。

定向测试覆盖 15,300 tick 交易日中的 tick 0、日末 15,300、次日开盘 15,301、开盘窗口末 16,200、连续竞价首 tick 16,201、再下一日开盘 30,601；另检查无开盘窗口的简化局 tick 5、严格 `minuteSlot` 仍抛错。保留原有非法输入断言。

## TDD 证据

普通 Node 验收使用 Node `v24.18.0`，单个 test 文件、`--test-timeout=10000`、`--test-concurrency=1`，并由 `scripts/run-with-deadline.mjs 10000` 从进程外监督。

先只加测试运行基线：失败 1 项（其余 4 项通过），新用例在 `currentMinuteSlot(15301)` 收到 `RangeError: 连续竞价 tick 15301 尚未结束开盘窗口`，调用栈为 `minuteSlot` → `currentMinuteSlot`。实现后同一命令通过，5 项通过、0 项失败；输出副本 `green-test.log` 在本目录（该日志受仓库 `*.log` 忽略规则忽略）。

限定 lint 尝试未能启动：工作树没有已安装的 `oxlint` 可执行文件，进程监督器报告 `spawn .../.tmp/node24-env/node_modules/.bin/oxlint ENOENT`。未运行浏览器、Cargo、构建或完整 Web 编译。

## 语义与范围

本改动只修复前端显示分钟桶时复用的交易日内时间映射，遵循 ADR-0009 的 15,300 tick 交易日和 900 tick 开盘窗口定义；不改变 engine 的权威交易阶段或实际交易规则。改动限于 `trading-timeline.ts` 和其定向测试。

## 基线文件 SHA-256

- 修改前 `trading-timeline.ts`：`c00e5abbb1b871238ed43b4985a3c9b8f4902bd7aae15d585043e182caafe3fe`
- 修改前 `trading-timeline.test.ts`：`0287d468754d1ec22fb895eceb4fcf8f9039ba839792097d0ed40aa8e705f048`

## 修改后文件 SHA-256

- `trading-timeline.ts`：`b8802f2af9611ff16e403d996081a9b364a91d07baf90a9e88393c549b92ce86`
- `trading-timeline.test.ts`：`a34cfbdb485f97c4157b3f4942ece250b27a1e3c0b80a09538374b0c60caf0a9`

`git diff --check` 通过。独立复核尚待未参与实现者完成。
