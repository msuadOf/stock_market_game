# React 渲染错误出口

G50 的真实入口在 `render-app.tsx`，`RenderErrorBoundary` 包裹 Provider/App，覆盖 React render 与 layout 生命周期异常；不宣称覆盖事件回调和异步异常。错误转为 `UI_RENDER_FAILED`，复用已有脱敏、复制反馈、刷新出口，刷新明确提示未保存日内进度会丢失，不自动读取存档。

最初两个 SSR/接线短测通过，非作者 `/root/web_gap_review` 发现直接读取 `Error.message` 会执行 getter，数字 message 会令 fallback 再次失败。新增两项反例在原实现确实失败，修复后四项 boundary 与八项已有详情测试全部通过，整命令约 2.34 秒，显式 `--test-concurrency=4`、case 10000ms 与外部 `timeout 10s`。descriptor 不可读、没有自身 message、访问器和非字符串均提供明确诊断，原 cause 保留；不静默 fallback，也不执行访问器。

同一非作者对四个产品/测试文件完整 diff 再审三门禁通过，没有改变 A 股、费用、日终存档或撮合语义。没有进行真实浏览器异常捕获或完整回归。前次类型编译通过；并行宿主/UI接口施工中的后续全项目编译尚待相应 owner 收尾，不将其称为本项通过结果。
