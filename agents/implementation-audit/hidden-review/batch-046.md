# 批次046：hosts来源复核

## 覆盖

基线 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。本记录复用已完成的逐篇全文读取，无需重扫：`agents/oop-refactor-audit/exhaustive/modules/hosts-01.md` 共67行，SHA-256 `3c547aacdad8ac3f52a1e3ad4b31b1e23c69fb7ed1136c8e4f2f966b6eae8fc7`，连续读至EOF；`hosts-02.md` 共63行，SHA-256 `baa7fcdf5932a48c7627a31a62cf2646f95bc3c6c60a8fd1fb6d5ab0ef123805`，连续读至EOF；`hosts-03.md` 共75行，SHA-256 `c60d43279a7085f21d6aa297fca2be9ce375eda7b58b31c4b76cd25f5797d104`，连续读至EOF。三篇章节族依次为：`按文件核销/聚合边界/调用契约/H01修订/迁移验证/阅读限制/单文件修订`；`已有对象化设计/候选聚合/文件核销/调用契约/迁移验证/未核实边界`；`模块结论/部署路由/健康检查/暂停安全/协议更新/发布器/WS/WASM/协议测试/跨模块边界`。

本次按要求在主工作区重新实测 `hosts-03.md`：`wc -l` 输出75，`sha256sum` 输出 `c60d43279a7085f21d6aa297fca2be9ce375eda7b58b31c4b76cd25f5797d104`（64位）。此前目标JSON的SHA录入有误，现以命令输出更正；此前Markdown值也少了 `fa`，现同步更正。

已读仓库 `AGENTS.md` 与 `docs/principles.md`，并参照 decisions（至 ADR-0028）、`docs/open-questions.md`、implementation-audit G/Q主账和 completeness hosts汇总。当前caller静态核验基线 `43b1aa5`：桌面 `SessionHandles.cmd_tx` 私有于 `apps/desktop/src-tauri/src/actor.rs:333-336`；服务端 `cmd_tx`、`event_tx` 私有并经 `subscribe_events()`返回接收端，见 `apps/server/src/actor.rs:606-625`；服务端WS初始订阅和重同步使用该句柄，见 `apps/server/src/routes.rs:1269-1275`、`:1414-1420`；WASM thread-local `REGISTRY`持有私有 `SessionRegistry`，创建/恢复/访问/步进/删除委托其方法，见 `apps/web-wasm/src/lib.rs:43-115`、`:331-350`、`:473-529`。

## 结论

历史候选 `hosts-01-A01/A02` 与 `hosts-03-A01` 在当前产品树已有实现，不应重复登记为待实施。`hosts-02` 的 routes 子模块拆分原文已明确是可选组织说明，不是OOP动作；不声称本批确认其实施。桌面 `tick_and_emit`仍以 `run_cycle(1)`逐tick推进，`pending_fixed_events`等陈旧字段仍留存（`apps/desktop/src-tauri/src/actor.rs:680-687`、`:832-887`），所以G19固定倍率聚合不因候选结构落地而核销。WASM `SessionRegistry::register`仍用 `NEXT.fetch_add` 后直接insert（`apps/web-wasm/src/lib.rs:57-62`），Q20句柄回绕覆盖风险仍未解决。`routes.rs:1256-1261`的WS注释声称仅处理存活/pong，但后续handler处理`Resync`，文案缺陷仍在。

不改变G/Q状态；OOP抽取不等于行为修复。未发现新ADR明确取代这些边界。没有产品代码改动，未运行测试、构建或回归；未重新核验交易所规则原文，本批没有提出A股规则变更。
