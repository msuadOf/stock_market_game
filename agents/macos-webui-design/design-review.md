# macOS Web UI 设计记录

## 依据与范围

2026-10-05 通过 Computer Use 查看用户已打开的同花顺 macOS 行情页。借鉴窄侧栏、紧凑行情表、右侧个股联动与底部状态条；不存储同花顺中的个人自选分组内容，不复制品牌或无关辅助工具。

已读 AGENTS.md、principles、architecture、testing、open-questions、ADR-0007、ADR-0027 与 trading-rules。变更只在 Web 展示层，不改撮合、价格、金额、存档、交易时段或数量契约，因此无新增交易规则法源判断。

## 组件归属

| 能力 | 既有 owner | 本轮处理 |
|---|---|---|
| 布局 | WorkspaceGrid / workspace-layout | 增加预设与恢复入口，保留拖拽缩放 |
| 行情选择/排序 | MarketGrid / AG Grid | 保留键盘选股与排序，列宽 flex |
| 图表/盘口 | ConnectedChartPanel / PriceChart | 保留数据与单位，只调整桌面间距 |
| 委托/存档/错误 | App 与现有 commands | 不改行为，顶部允许换行 |
| Select | Blueprint HTMLSelect | 保留原生弹出选择 |
| 滚动条/主题 | index.css | 引用现有 token |
| 账户摘要 | DesktopAssets | 移至桌面底部，复用原有金额计算 |

## 设计自检

采用行情工作台而非营销式大卡片。全部面板保留真实游戏数据，不增加假行情或装饰指标。移动端不参与本次桌面视觉变更。现有 DESIGN.md 中部分早期色值与 index.css 有差异，本轮不扩展主题治理范围，桌面完全引用既有 runtime token。

## 验证记录

- 新预设测试首次在依赖未安装时因缺 react-grid-layout 失败；依赖安装后再次运行，确认因缺 createWorkspacePreset 导出失败（有效 RED）。
- 实现后预设及原布局 6 个测试通过，进程树 deadline 10000ms、case timeout 10000ms、test-concurrency=2。

## 实际验收补充

- React 18 类型验证单独拆分后 1/1 通过（3.25s，10000ms deadline），首次整组受冷启动及并行编译影响超时，未放宽上限。
- 严格 premium audit：0 error / 0 warning。DESIGN.md 官方 lint：0 error，8 条既有 token 未被 component 引用的 warning。
- 全 Web unit 使用8进程分片，失败于未改动的 wasm-worker-ownership 两项 HostFailure.message fixture；独立 reviewer 已复现。其余分片取消，不标为全量通过。
- 全 Web lint 失败于既有测试文件5处 no-children-prop；本轮改动文件单独检查另记。
- 全浏览器回归使用2 workers、300000ms外部共享deadline：11通过、3失败，50.4s。新桌面2项均通过，包括1440×900、844×390、390×844以及布局切换后的图表与委托输入保持。其他失败为公司报告预期日期/公开编号及异步暂停偏好（相关逻辑未改，未做 HEAD 对照），详见 e2e.log，未弱化断言。
- 生产WebUI已在内置浏览器启动并实际显示；横屏桌面、竖屏移动按宽高比切换。真实局首次推进出现“日终存档不能包含未处理的日内请求”的运行错误（来自未修改路径，尚未完成根因修复），保持错误可见，未隐藏或改写存档。游戏已暂停以便用户查看。
- 视觉检查发现固定代码列被截断，已增加为110px最小宽度；侧栏“走势图与盘口”短标签改为“个股”，完整可访问名称保留。

## 最终交付

- 最后代码列与 Label in Name 修正后，本轮全部改动 TS/TSX/E2E 文件定向 oxlint 0 问题。
- 最终 desktop E2E 首次执行遇到 10000ms 总deadline，保留 desktop-e2e-final.log；环境稳定后重跑同一断言与同一上限，2/2通过、2 workers、2.7s（desktop-e2e-final-retry.log）。
- 最终制品 final3 已实际在内置浏览器重新加载并启动；完整股票代码和“个股”导航已目视确认，游戏保持暂停。桌面截图为 desktop-preview.jpg；临时 viewport override 已恢复，浏览器按实际窗口横竖屏自适应。
- 独立 reviewer 发现的 Label in Name 已修复并再次复核；无未解决的本轮实现 finding。完整回归与日终保存限制仍按上述记录保留。
- 编译 Subagent 已按用户要求结束；production WebUI 为独立 detached 服务，不依赖 Subagent 存活。
