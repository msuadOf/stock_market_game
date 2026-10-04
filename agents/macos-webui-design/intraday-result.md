# 桌面分时修复与热更新验证

- 2026-10-05：3000 从 Rust 静态制品服务切到 Vite 开发服务，PID 见 `webui-service.pid`，日志 `webui-dev-service.log`；仅监听 127.0.0.1。
- 浏览器实际 viewport 1020×833，DOM 确认 `/@vite/client` 已载入，日志确认 LocalRefreshViews 和 App 的 HMR 更新。修改 hook 触发 App effect 重建宿主，不能承诺任意修改都保留盘中状态。
- 真实 WASM/NPC 运行到第1日 09:42:02，tick1622，300260 已有13个连续分钟点，横坐标16～20.2176，未来时段留白。暂停并恢复1x，截图 `intraday-fixed-1020x833.jpg`。
- 18项分时/投影/runtime短测、3项测速短测通过；case和命令树10000ms。TypeScript构建通过；修改文件oxlint通过；4项桌面E2E以workers3通过，使用长验收共享300000ms deadline。premium strict audit无finding。
- 独立审查发现MACD量程和昨收精度问题，已补失败测试并修复，复核通过。详见 `independent-review-intraday.md`。
- 本轮未重跑全部项目回归；此前公司资料日期、移动checkbox的失败不在本次修复范围。页面底部真实NPC委托被拒提示仍保留，没有隐藏错误。
- 先前生成的 `webui-macos-20261005-intraday` 是切换到开发模式前的中间制品；最终源代码以当前Vite服务为准，不宣称该制品包含后续全部修正。
