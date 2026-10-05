# 共用交易面板实现与验收

日期：2026-10-06。用户已授权实施、commit 与 push。参考观察与用户确认见
[观察记录](reference-trading-interactions-2026-10-05.md)。参考游戏仅提供布局与交互依据。

## 实现范围

- `QuickTradingPanel` 与 `QuickTrading` 是横竖屏共同的表单与业务 owner；桌面同时显示卖出、买入，手机切换方向。各股票、方向独立保存草稿。
- B1、S1 使用真实报价；手工输入、价格加减退出跟随。最小、最大继续提交 Lowest、Highest，由 engine 受理时解析；显示的费用预留边界不称作确定成交价。
- 盘口价格与手数分别为语义按钮。价格只改变对应草稿的价格；手数累计较优档位，舍尾为整手，并受可买／可卖资源限制。
- 快捷委托输入手，精确转换成整数股交给既有校验。比例以可用整手为基数，结果可以为0，但不能提交0。手工小数手保留合法零股卖出，T+1、已预留量、费用与类别单笔上限继续生效。
- 自动按游戏时间执行，绑定原股票与方向。不足保持开启并显示等待原因；暂停不执行，关闭自动保留挂单。取消所有查询本人所有证券的活动委托，不关闭自动。
- 自动间隔最小1游戏秒；合并交付跨过多个间隔时只用最新权威状态执行一次，不补造过去的委托。会话重新同步停止自动，明确提示重新开启；这些游戏简化已登记正式契约。
- 不实现批量交易，不新增依赖或 engine 协议。桌面 dock 默认320px，低高度240px；共用副图最低28px，防止坐标重叠并保留底部按钮。

## TDD 与独立复核

先用失败测试固定草稿隔离、盘口价量区别、比例零手、自动时序和全证券撤单，再实施。
独立复核发现的暂停等待 guard、极大金额费率1精度、symbolic 价格误导已修复。
后续全新 `gpt-6.1-sol high` 复核发现的三个P2均先补失败测试或同步契约后修复：

1. 缺少 opening 阶段的自定义局跨日曾额外跳900秒。四种 opening／closing 组合现验证相邻1游戏秒及10秒自动到期。
2. 未定义的焦点 token 改为既有 `--focus-ring`。浏览器验证实际 outline 为 solid、2px。
3. DESIGN 旧数量单位和几何约定同步到当前实现。

最后由另一名全新、未实施改动的 `gpt-6.1-sol high` 独立复核完整 diff 与新增文件，
未发现新的有效P1/P2，确认大A语义、最小范围和边界处理，通过代码复核门禁。
该 agent 独立短测15/15通过。官方制度依据及尚未重新访问成功的范围保留在 `docs/trading-rules.md`，不把参考游戏或访问失败当成制度证明。

## 验证结果与限制

- 最终普通测试：156个文件，862/862通过；8个并发进程，检测到10个CPU；外部10秒 deadline 内2.711秒完成。一次资源竞争重跑曾超时，保留日志，不放宽断言或时限。
- 类型检查、production build、release WASM 校验通过；Vite 构建使用其并行构建能力。
- 改动相关 production 与新 E2E 文件 lint 通过。全项目 lint 仍有原有5条 `no-children-prop`，分布于已有 SSR 测试、MobileStockDetail 与 workspace-grid，不将其描述为通过。
- 完整浏览器批次以3 workers、每页面2个 WASM 线程运行，70/72通过。两条刷新读档用例在10秒总 case 限额内超时；trace 显示启动、推进与重新加载消耗了大部分限额。
- 这两条读档用例在单页面、WASM双线程隔离复测中分别7.2／8.0秒通过。新增五条交易用例全部通过，包含键盘焦点、窄竖屏、symbolic 文案、真实盘口点击、自动原证券绑定与撤单后保持开启。隔离通过不能表述成完整批次72/72通过。
- 原坐标几何断言全部保留，展开交易栏的周K坐标、按钮与横屏无溢出场景通过。异步撤单 E2E 先等待全部请求已提交，再推进 tick，不通过弱化结果断言规避竞争。
- frontend premium strict audit 0 findings，证据位于本目录 `quick-trading-premium-audit.json`。当前962×833桌面及320×844手机在用户内置浏览器截图验证，临时视口已恢复，游戏暂停展示。

## 证据

- [桌面截图](quick-trading-desktop-current.jpg)
- [手机截图](quick-trading-mobile-320.jpg)
- 本目录的 `quick-trading-unit-release-isolated.log`、`quick-trading-production-build.log`、`quick-trading-full-e2e-final.log`、`quick-trading-restore-isolated.log` 和 `quick-trading-review-fixes-e2e.log` 保留命令结果；日志按仓库忽略策略不加入 Git。

只提交本轮代码、契约与工作记录；仓库原有的其他未跟踪截图和排查文件不纳入本次提交。
