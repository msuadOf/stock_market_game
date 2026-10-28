# V11 公共桥初始 fixture 接缝与真实正控

2026-10-27。本批只准备公共桥可接受的初始输入，不构建或执行 WASM，不修改生产恢复门。

## 具体约束与合法装配

原九项测试使用 `GameSession::restore`，允许尚无自然日日结的合法初始 SaveSlot。实际 `restore_json` 则经 WASM `SessionRegistry::restore → ProtocolSession::restore`；后者要求 `settled_through.is_some()`（fault3013）。不能以 GameSession 正控外推公共桥，也不能删除3013、手填settled或新增测试专用WASM API。

独立 exporter 保持原9case/helper源码不变：

1. 从相同两股零NPC规格重新 `GameSession::new`，真实 `start_date=2030-01-01`，不挪动已运行局的时钟。
2. 首次 restore 前冻结同一 synthetic SSE/2030覆盖Jan1..Jan2。Jan1 SSE与SZSE均闭市，初始tick/day为0。
3. 真实初始 `GameSession::restore` 后调用一次 `end_civil_day`，由生产逻辑生成 `settled_through=Jan1/current_date=Jan2`，没有任何市场tick。
4. `save()` 后真实调用 `ProtocolSession::restore` 正控，验证tick/day仍为0、当前Jan2。这个JSON供最终native Protocol与实际wasm桥共同读取；之后mixed市场只在内存运行。

该装配不要求C4的mixed跨日市场恢复，不更改已有前史，不切换运行后的source/policy，不伪造市场事实。setup每次从新局重建，StockId仍由catalog确定。

## 已执行与绑定

- 使用 **6ac167e1** 的root授权normal rlib，SHA `9bee5bf8bb46234eaaeb075f82d38dde14ae2c4e9169664669f1627ed88f1480`。当前main已有后续Monthly窄改，本批不能冒称其同源构建；零NPC正控仅证明此公共装配可恢复，不外推后续估值行为。
- exporter source SHA `3a387346750f5a8af6eaac3f23c26feaefd0b0dc6258f7da9f130217837a62fa`。helper仍 `d1120a2ad62e3d0c1ca15ec1126ec44cf2050b48c4f84cb07e4dafbb5f4b2eb3`。
- 轻编译rustc codegen8/lld8、进程外10000ms，exit0，1.180290秒；binary39014096 bytes，SHA `3788ab13961040adf4e775f63d621cdc4bb5e83e346971ba3688de2a64786965`。
- 正控命令在执行前已写入 `wasm-public-fixture/compile-plan.json::run_outer_argv`；实际外层10000ms、exit0、1.686742秒，完整stdout/stderr与run-result保留。
- 实际生成JSON：根 `.tmp/mixed-calendar-c2c/public-wasm-initial-6ac.json`，2321588 bytes，SHA `63d03f04a821ba9a84d8e35d20f2c032415afd95d0dca5d085ad37f00f3fb1ae`。只生成独占新文件，不覆盖旧输入。

本批没有native/wasm对拍结果。最终生产green必须基于实际staging源重新构建native库与wasm/glue，再绑定同一输入和runner；不复用本旧库宣称中央已完成。
