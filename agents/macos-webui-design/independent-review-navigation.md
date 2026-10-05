# 同花顺式桌面导航独立复核

- 日期：2026-10-05。
- 复核者：未实施本轮变更的 independent_review subagent。
- 范围：当前完整 tracked diff，以及新增 DesktopTerminal.tsx、desktop-terminal.css、desktop-workspace.spec.ts；参考本主题导航与验证记录。未启动构建、服务或测试，未修改实现；本文件为唯一新增审查记录。
- 基线：AGENTS.md、principles、architecture、trading-rules、相关 ADR、DESIGN.md、UX-CONTRACT.md 和两份 frontend-design skill。用户本轮明确要求同花顺式桌面操作层次，替代上一轮仅调整自由面板的方案。

## 复核结论

此前提出的 4 个问题均已在最终源码中解决，没有新增阻断性 finding。实现可继续进行最终浏览器与静态制品验证；本复核不能代替未完成的最终测试，也不代表整个游戏回归通过。

### 已修复问题

1. **当前个股交易目标错误。** App 向 WorkspaceGrid 传入 `onTradeCurrent={() => selectStock(chartCode)}`，DesktopTerminal 的两个个股交易入口先调用该动作再打开交易页。代码与最新价同步当前图表证券；数量、委托类型等草稿保持。侧栏“交易”仅切换页面，保留原草稿。交错 E2E 明确覆盖委托先选 002156、图表仍为 600101 时当前个股入口纠正代码且保留数量 300。
2. **公司与个股上下文漂移。** 桌面 ConnectedCompanyPanel 使用 stockContext，直接按 chartCode 映射公司，不受旧 selectedCompanyId 覆盖。桌面隐藏独立公司选择器，移动路径默认行为保持；无映射返回 null 并显示已有明确空态，不沿用前一公司。E2E 覆盖 F10 进入 C-600101 后从左侧切换至 C-002156。
3. **自定义往返重挂载。** 自定义入口已移除，DesktopView 限定行情、个股、公司、交易、游戏 5 页；固定 panels.map 保持 section key 与内容实例，通过 hidden 切换。行情预览标题虽条件显示，但 panel 位于稳定子位置；切页不重建图表与公司组件。旧 WorkspaceDesktopLayout 仍保留独立测试，不作为当前导航路径。DESIGN/UX 已明确此项受用户最新要求替代，未继续承诺不可用的默认自由布局。
4. **F10 虚假承诺。** 已增加桌面 keydown effect，处理 F10 并 preventDefault，排除修饰键及 input/textarea/select/contenteditable 输入位置；卸载清理 listener。公司资料按钮提供 aria-keyshortcuts。竖屏不挂载 DesktopTerminal，因此不会接管移动页面快捷键。

## 大 A 语义与必要性

- 所有新增行情列读取现有权威 MarketSnap；昨收、买一、卖一采用 Cents 格式化与金额比较，空买卖盘显示缺失，不补造价格或成交量。
- 盘口显示元/手，委托输入继续以股为单位；资金持仓继续复用原有 usePortfolio，底部账户摘要复用 DesktopAssets，没有新增取整或 Number 金额路径。
- 市场逐笔成交明确标为市场成交，不冒充玩家成交；撮合、T+1、费用、存档 schema 和日终候选代码均未修改。
- 未新增交易制度判断，因此本轮无需另立交易所规则依据；没有把 UI 调整包装成现行交易制度重新验证。
- 行情→个股→公司资料、交易及游戏管理层次符合用户新需求。存档/新游戏/暂停偏好/远程刷新模式集中进入游戏管理，保留真实动作。旧 RGL 库未在本轮顺手删除，避免扩大依赖迁移范围。
- 新样式主要以 desktop-terminal/layout-desktop 作用域约束，竖屏 WorkspaceGrid 仍直接返回原 app-grid，移动详情及交易底页路径保持。共享 CompanyPanel 的新参数默认 true，移动原独立公司选择不受影响。

## 隐藏状态与图表

- hidden 面板显式 display:none，不留可见或可聚焦的隐藏内容；同一 panel 实例保持其 React 本地状态。交易查询 dock 独立保留在 DesktopTerminal，切换其他页后仍保留选择。
- PriceChartRuntime.resize 同时按可用宽高更新主图/指标图；宽高为零时保留上一有效尺寸，避免切到公司/交易页时将图表破坏为零尺寸。返回显示后 ResizeObserver 再更新。新增单测覆盖高度扩展与隐藏，既有释放/监听测试断言没有弱化。
- PriceChart 指标按钮新增 aria-pressed，可直接验证状态保留；新版 E2E 已补“无”指标经公司页往返仍保持的断言，最终执行结果仍待主 agent 更新。

## 测试变更与证据界限

- `git diff --check` 通过。
- 已读取 workspace-render-final.log：类型与 render 6/6 通过。原自由布局 render 测试改为直接测试保留的 WorkspaceDesktopLayout，断言仍覆盖 6 面板拖柄/缩放柄；产品默认入口变化由新增导航 E2E 覆盖，非删除断言掩盖失败。
- 已检查 company-information、mobile-layout、trading-workflows E2E diff：更新为真实桌面导航及游戏管理路径，保留原编号/日期、资金、冻结、存读档和暂停偏好断言。未降低领域断言以制造通过。
- 主 agent 报告本轮 UI 单元测试 10 项通过、新导航 3 项已在一次全套运行通过；本复核不冒称亲自运行这些测试。
- 读取到的 navigation-final.log 是旧失败轮：2 pass / 1 fail，曾出现定位股票 select 超时与进程树清理未确认；不能把该文件名中的 final 当作最终成功证据。主 agent 正进行静态 E2E 制品复测，交付应引用最新真实日志，并明确其结果。
- 第一轮全套 E2E 的旧编号/报告日期与移动暂停偏好失败、此前生产局日终存档错误仍是已知验证限制，不能因导航通过而抹除。最终结果应区分本轮导航功能验收与整个游戏的遗留问题。

## 非阻断后续验证建议

最终浏览器验证确认 844×390 交易/游戏页全部操作可达、个股大图返回预览尺寸恢复、F10 在输入字段不抢键、隐藏页面往返保持行情排序与财务阅读状态。现有源码结构支持这些契约，但有限静态复核不替代全部运行时证据。
