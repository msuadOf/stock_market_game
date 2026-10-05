# 暂停偏好浏览器操作独立复核

2026-10-05，基于 HEAD 8b8c04b 的完整 working diff，排除服务 PID。复核者未实施代码，未提交。

## 初审

核心修复方向正确：显式用户修改不应被 tradingE2EMode 静默跳过。UI 保持宿主确认后才发布 settings 与 sessionStorage，未改 engine 暂停屏障或交易规则。

1. 大 A 语义：不修改开盘/收盘制度、交易阶段、撮合、结算或资金单位。只是恢复游戏已有暂停偏好的真实操作通路，无新增交易制度，沿用既有领域契约足够。
2. 必要性与范围：App 与 hook 移除 UI E2E 跳过直接针对故障，启动 E2E 的受控停止/单步流程位于 useSessionHostLifecycle，不受此变更影响。新增 sessionStorage 两次断言覆盖两端操作结果，没有弱化已有 checkbox 要求。
3. 边界与复杂度：保留 host null、确认前不发布、旧宿主晚确认/拒绝、错误显式报告及队列恢复。发现低优先级复杂度问题：synchronize 的 skip 参数已无生产调用需要（唯一生产调用固定 false，true 仅旧测试），启动流程也不调用该方法。建议删除死参数、死分支及仅 skip 的旧断言，保留其他有效异常/确认断言；已告知主 agent。

独立执行 pause-preferences-runtime.test.ts 与 config/pause-preferences.test.ts：9项全部通过，约0.11秒，case/整命令10000ms、并发3，使用 run-with-deadline.mjs。

核对 pause-preferences-ui-green.log 时实际为1通过1失败，afterClose.check()报点击未即时改变状态。该日志文件名不是通过证据；已告知主 agent。异步宿主确认的受控 checkbox 可用 click 后 expect.toBeChecked 验证最终确认，不能通过改成未确认的乐观状态绕过领域契约。待主 agent 修复测试同步方式、处理死参数后再次复核；本轮不提前声明完成，更不宣称整个终端目标完成。

## 最终再次复核

完整最终 diff 共四个源码/测试文件：App.tsx、usePausePreferences.ts、pause-preferences-runtime.test.ts、mobile-layout.spec.ts。初审建议已处理，未发现新有效 finding，本批独立复核通过。

- synchronize 已删除不再有生产用途的 skip 参数和分支，isCurrent 参数所有调用均正确迁移；启动 E2E 流程未改。旧测试仅删除已废 skip 专属调用和断言，host null、宿主失败、storage getter/写入失败、确认前不发布、旧宿主隔离及拒绝后队列恢复仍保留。
- E2E 用 click 后逐步等待 checkbox 最终状态替代要求即时状态的 check/uncheck，符合原本 actor 异步确认契约；没有使用乐观 UI 绕过确认。两次 sessionStorage 精确内容断言继续验证手机开启、桌面关闭一个选项的落盘结果。
- 独立重跑9项短测全部通过，约0.108秒；case与整命令10000ms、并发3。git diff --check 通过。核对 pause-preferences-ui-final.log 为2项通过、2 workers，8.1秒，暂停偏好用例1.7秒。先前同名含green但失败的日志保留为排查证据，不混淆最终结果。

大 A 语义、必要最小范围与错误边界门禁均通过；构建/lint 由主 agent 后续执行，未预先认定其通过。本次仅关闭暂停偏好这一已定位问题，不据此宣称其他公司报告、活动委托/存档问题或完整终端目标已完成。
