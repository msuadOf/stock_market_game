# 行情 UI 最快档性能回归

脚本用本机 Chrome/Edge 的 DevTools Protocol 驱动真实页面，不引入项目级 E2E 框架依赖。它会：

1. 复用 `--url` 指定的页面；页面未启动时自动启动 Vite。
2. 使用 390×1180 移动端视口，进入第一只股票详情并切到“最快”。
3. 分别观察分时图与日 K，验证权威游戏 tick 和图表签名都在推进。
4. 采集 Chrome 的任务、脚本、布局、堆内存、DOM 节点、长任务、布局偏移和近似 FPS。
5. 生成 `report.html`、`report.json` 和四张前后对比截图。

运行：

```bash
pnpm test:market-performance
```

可选参数：

```bash
pnpm test:market-performance -- --duration-ms 10000 --url http://127.0.0.1:5173/ --output artifacts/my-run
```

报告默认位于 `artifacts/market-ui-performance/<时间戳>/`。任何一个推进检查失败，进程都会返回非零退出码，并在初始化失败时写入 `failure.txt`。
