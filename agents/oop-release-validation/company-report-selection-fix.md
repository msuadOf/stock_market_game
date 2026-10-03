# 公司公开报告选择刷新回退排查

日期：2026-10-03。本文件为本轮工作记录，不替代正式交易或财务规范。

原始浏览器验收 `e2e.log` 为 12 个场景中 11 个通过，平板报告切换失败。完整读取
`apps/web/e2e/company-information.spec.ts`、错误 context、trace 与公司报告相关源码后，
确认是产品选择状态的刷新问题。trace 中点击在约 38943ms 确实把半年度报告按钮
设为 `aria-pressed=true`，公开编号 9 断言通过；约 39135ms 又恢复为季度报告编号 4。
这不属于点击没有触发。原失败 trace 保留于 `.tmp/oop-release-e2e-failed-trace/`。

`CompanyQueryCoordinator.acceptCivil` 在自然日更新时强制刷新已查询公司；
`startCompanyQuery` 将 root page 设为 loading，`visibleReports` 暂时返回空列表。
`CompanyPanel` 原选择协调 effect 不区分临时 loading 与真实 empty，因而把用户
选择清成 null。恢复相同报告集合后，面板回到默认首份报告。两页并发诊断的首次
查询各观察到公司报告重复刷新，后续稳定窗口中 30 次选择均保留。

最小修复仅让 `CompanyPanel` 在 ready 或 empty 时协调报告选择。loading、error、
unavailable 与 idle 不凭临时空列表清除选择；ready 时失效编号和真实 empty 的
既有回退仍保留。E2E 改为点击真实报告 button，并补查按钮 pressed 状态及键盘
导航后报告期间；原编号、期间、公开内容与表格断言全部保留。

新增组件行为测试复用项目 memoryHook，并显式模拟提交后的 effect；不把它描述为
真实 DOM 测试。覆盖 loading/error 后仍可见选择、真实 empty 后重新出现报告时
回退、切换公司后失效编号回退。跨公司同字符串编号的既有匹配规则没有在本批改变。

## 验证证据

- `company-selection-red.log`：新增回归正确失败，预期选择 8，实际回到 7；其余两个
  边界通过。
- `company-selection-green-reviewed.log`：修复及独立复核修正后，三个公司展示套件
  13 个 case 全部通过，800.98ms。case/进程树均 10000ms，文件并发 2。
- `company-selection-tsc.log`：首次 fresh build 暴露测试 memoryHook 初始 props 的过窄
  泛型推断。已显式指定组件 Props；`company-selection-tsc-fixed.log` 对应直接
  `tsc -b`（app 与 node/E2E 配置）退出 0。此前错误工作目录调用及短编译 deadline
  失败不算通过证据。
- `e2e-fixed.log`：12 个通过，但复用了诊断遗留 preview，不能作为修复后构建证据。
  已终止该 PID 1575179，并确认端口 4187 无监听；诊断脚本同步修正进程树清理。
- `e2e-fixed-fresh.log`：首次强制 fresh build 因上述 TypeScript 错误退出 2，未运行
  浏览器 case。
- `e2e-fixed-fresh-final.log`：CI=1、独立端口 4189、workers=2、retries=0；外部
  `run-long-validation.mjs 300000`，共享 deadline 包含构建与 preview 清理。新构建
  与 preview 启动记录明确，12 个 case 全部通过，40.8s。运行中两个 test worker
  各约 26% CPU，WASM renderer 约 230%/298%，确认真实多核使用。

本批不改 A 股交易制度、金额单位、报告公布时点、财务事实或私有信息可见性；沿用
既有公开 DTO 和领域规则，不新增需要外部交易规则查证的制度。独立复核指出公司
切换 fixture 的 `financials.scope` 仍为旧公司，已同步到新公司并重测；最终独立
结论由本轮 reviewer 记录。
