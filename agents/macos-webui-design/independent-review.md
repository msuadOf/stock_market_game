# macOS WebUI 独立复核

- 日期：2026-10-05。
- 复核者：未实施本轮 UI 的 independent_review subagent。
- 范围：当前全部 tracked diff、新增 desktop-terminal.css、workspace-presets.test.ts、desktop-workspace.spec.ts，以及本主题构建与设计记录。日志和构建制品仅作为证据读取，不作为实现源文件。
- 已读：AGENTS.md、principles、architecture、open-questions、trading-rules、相关 ADR-0025/0027、两份 frontend-design skill 及 verification checklist。

## 当前结论

最终源码复核已完成。发现的侧栏可访问名称问题已修复并复核，当前没有未解决的本轮实现 finding。设计预览与本地构建可以交付，但全量回归失败及真实生产局日终存档错误必须显著报告；不能称为完整游戏验收通过。后续实现变动须再复核。

1. **大 A 语义保持。** 没有修改 engine、Host 协议、价格/资金运算、申报数量、T+1、存档或订单校验。账户摘要复用 DesktopAssets；行情手与委托股提示符合现有显示/输入边界。没有引入新的交易制度判断，因此无需为未变规则伪造新法源核验。启动页 noValidate 仍由既有 resolveStartupTarget/validateRemoteServerAddress 显式拒绝空或非法地址，不会静默 fallback。
2. **范围必要且集中。** 同花顺式工具栏、侧栏、列表与个股布局、底部账户摘要均对应用户设计目标；沿用既有 token 和组件，无新增依赖、商业辅助工具、真实行情、账户系统或公开部署。两种 preset 是同一布局模型上的有限扩展。原生 date 归属登记属于现有能力说明，无新增日期行为。
3. **状态与滚动边界。** preset 只替换 layouts，WorkspaceDesktopLayout 和面板 key 保持稳定；没有通过 preset key 重挂载游戏组件。面板内容沿用既有 overflow:auto，新增 workspace-scroll 管理画布。save-group 的桌面选择器覆盖原窄屏隐藏规则；竖屏仍走原 app-grid。实际导航、resize、窄窗口和状态保留必须由浏览器验证确认。

## 验证与已知限制

- `git diff --check` 通过。
- 独立执行 `node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=2 apps/web/src/host/wasm-worker-ownership.test.ts apps/web/src/app/workspace-presets.test.ts`：preset 2 项通过；既有 wasm-worker-ownership 2 项失败；测试子命令退出码 1，约 148ms。日志见 `independent-targeted-tests.log`。
- ownership 失败均为 `ProtocolError: HostFailure.message 必须是非空字符串`，对应原测试 93/137 行。相关测试、wasm-worker、protocol-failure 与 fixture 不在本轮 diff，失败依赖链不导入此次 UI 变更；独立复现与主 agent 全套测试日志一致。这不是本轮布局引入的直接依赖失败，但全套 Web 测试仍必须报告为失败，不能记为通过。
- 主 agent 报告全局 lint 既有 5 处 `react/no-children-prop`。所述 local-amount-render、workspace-grid、mobile-component-render 测试均未由本轮修改；独立复核未重新运行全局 lint，不能声称独立验证了全部 lint 输出。
- 已建议主 agent 补 `844×390` 或 `800×600` 窄横屏验证：新增 `max-width:900px` 分支将 root 恢复为固定高度，现有新 E2E 的 `1000×700` 未命中该分支。检查顶部换行后面板滚动、存档和账户摘要可达。
- 构建记录在本次复核时尚未补最终部署状态；完成后需要记录正式构建结果、实际服务地址、重启方法与准确日志路径。当前复核不把准备 Cargo 缓存当成正式制品构建成功。

## 最终复核补充

- 再读全部 diff 与新增源码，核对代码列最小宽度 110px、侧栏“个股”短 label、更新后的 E2E 及部署记录。代码列调整仅影响桌面 AG Grid 几何，不改证券代码或数值格式。
- **已修复 finding：** 侧栏可见 label 为“个股”，原 aria-label 为“走势图与盘口”，可访问名称未包含可见文字。主 agent 修正为“个股走势图与盘口”，复核源码确认已解决；其他导航名称不变。
- `e2e.log` 记录 2 workers、14 项中 11 项通过、3 项失败；其中新增 desktop-workspace 两项通过（1.7s、1.5s），覆盖 1440×900、844×390 与 390×844。已确认测试源中的窄横屏宽度更新到 844，命中新 CSS 分支。preset/恢复的图表周期及委托输入保留、面板导航和存档可达有浏览器断言，前轮窄横屏证据缺口已补。
- 两项 company-information 失败为期望公开编号/发布日期与真实内容不匹配：期望“公开编号 1”而实际为 2；测试还寻找不匹配日期的半年度报告按钮。日志显示实际内容完整存在，失败不是元素被布局裁剪。报告数据、选择与生成代码未改。本轮没有通过更改领域数据或弱化断言掩盖失败。
- mobile-layout 暂停偏好失败发生于 390px 竖屏、切换桌面之前；Playwright 成功找到可见、enabled、stable 的 checkbox 并点击，但状态未改变。偏好请求逻辑与移动组件未改，当前没有证据将其归因于桌面布局。未运行 HEAD 对照，不能绝对声称已证明旧版同样失败；它仍是未修复的回归失败。
- 主 agent 在生产 IAB 实际启动游戏后，从默认 2030-01-01 推进至 01-02 遇到“日终存档不能包含未处理的日内请求”，已暂停并保留告警。错误源 `apps/web/src/save/day-end-candidate.ts` 不在本轮 diff；这是真实运行限制，不能用交易 fixture 的日终保存 E2E 通过来抵消或声称整个游戏无误。
- 构建记录现已补齐正式 `webui` 产品打包、回环 127.0.0.1:3000、HTTP 200、COOP/COEP 和入口资源检查。已读取 `frontend-final-build.log` 与 `product-final-build.log`，确认后续生产 Vite/TypeScript/WASM 检查和 `webui-macos-20261005-final` 打包成功。最终可访问名称修改发生于后续，因此主 agent 仍需将最终源码重构建后再指向交付制品。
- 初轮 WASM/CLI 构建超过 5 分钟且内置 supervisor 未终止，部署记录已如实披露；手动终止后增加 GNU gtimeout 的重跑成功。违反首次 deadline 的事实不能抹除，改用进程外监督后的成功只证明重跑结果。
- 主 agent 报告本轮全部变更 TS/TSX/E2E 文件定向 oxlint 为 0 问题；全局既有 lint 失败仍保留。最终 `git diff --check` 再次通过。
