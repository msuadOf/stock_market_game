# 隐藏扫描批次 079

## 范围与来源完整性

- 按 scan-plan 的 batch 79、owner 4 复核三份历史审计文；产品证据限定为 `.worktree/implementation-reaudit` 的基线 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。没有改产品代码、运行测试/构建或执行 Git 写操作。
- 三份来源均连续全文读至 EOF；实测 `wc -l` 和 `sha256sum` 与 scan-plan 相符。逐份内容包括前置复核与后续 delta/结论，不能把较早的“未实施”描述脱离后续记录当作现状。
- 已读取基线 `AGENTS.md`、`docs/principles.md`、`docs/testing.md`，以及相关 `ADR-0017`、`ADR-0028`。工具审计不改变沪深交易规则；`ADR-0017` 明确历史稳定字节/跨 worker 全产物比较不是交易优先级契约，`ADR-0028` 明确发布链 build-only，不以发布成功代替测试或线上验证。

| 来源 | 行数 / SHA-256 | 全文主张和当前复核 |
|---|---:|---|
| `agents/oop-refactor-audit/exhaustive/reviews/tooling-02.md` | 68；`f211b4dbbfb0ab8b3c2fb524321a1f13f431e48305c13e15ffbdf2c329a8e905` | 读至最终 closure resolution。其关于静态 Web 打包失败可能遗留部分输出、CDP pending 请求断连不 settle、进程停止未确认子树退出的历史线索仍需按源码边界理解；closure resolution 已修正 package-distributions helper 纯函数分类的关系记录，不是运行时缺陷已修复的证据。OOP 调查本身明确不要求把工具抽象对象化。 |
| `agents/oop-refactor-audit/exhaustive/reviews/tooling-03.md` | 48；`74c6e6bafa459d1598412e48bc2dcf7e8e1cf2346483e11efe3350f9fc84bbb2` | 读至 EOF。记录 server-build.sh 的硬 KILL 可能截断清理、Windows `taskkill` 异步返回及 `run-with-deadline.test.mjs` 缺 Windows 树退出覆盖；这些属于历史工具监督证据，不等于相应代码已修复或平台验收通过。8 个 `it` 的更正与来源一致。 |
| `agents/oop-refactor-audit/exhaustive/reviews/tooling-04.md` | 65；`d60ba828c953aa8990793d8f936860e1089e0343cae18919a914781e01aa1443` | 读至最后分类 delta。正文区分 `baseline-run` 逐 seed raw/receipt/aggregate 次序、sampler rejection 清理缺陷、候选与实现状态；后续版本已校正 sibling 并行恢复、`write:false` 测试覆盖和有界 cleanup 的表述。最终分类将 sampler 问题独立列为 defect lead，而非 OOP action，分类准确。 |

## 基线调用链与反证

- **tooling-02：** 基线 `scripts/package-static-web.mjs` 仍先修改输入树，再创建最终输出目录并逐个写 archive，失败时可能留下输入/输出部分状态；`scripts/performance/market-ui-report.mjs` 的 `CdpClient` 维护 `pending`，`close()` 只关闭 socket；进程停止路径仍需区分 OS/平台清理与目标树退出确认。因此来源中的运行时风险没有被 OOP 分类 closure 消除。它们分别关联已有性能/监督总账边界，不能因为 owner 类已存在便核销。
- **tooling-03：** `docs/testing.md` 规定普通 Node case 与命令各自 10 秒、长阶段 300000ms 并留清理时间；来源中的 server-build 与 Windows `taskkill` 问题是实现是否满足这些已批准边界的具体证据。其测试计数修正只是报告准确性，不是测试通过声明。
- **tooling-04：** 当前基线 `scripts/simulation/baseline-run.mjs` 已有 `SimulationBatchContext`；after/sensitivity callers 已通过它共享批次资源。因此较早把上下文写作待实施候选的段落已被同一来源后续元数据/分类 delta 反证，不能登记为当前缺失。逐 seed raw → receipt → aggregate 与成功 sibling checkpoint 的恢复事实仍成立。`escrow-performance-harness.mjs` 中 `ProcessSampleRun.start` 在等待 child close 后才 await sampler，sampler 先 reject 时未先触发 child tree 清理的缺口仍存在；对象提取与资源失败路径是不同责任。
- 主审计的 `G61` 覆盖性能工具整体 deadline、CDP/采样异常收尾；`G62` 覆盖嵌套进程树期限监督。三篇文提供对应既有问题线索，没有足够证据新增独立 G。`G39` 是 K7/after/sensitivity 对自由调度结果的验收契约；不能用工具对象调查、相同 stdout hash 或跨 worker 完整 artifact equality 将其核销。`G63` 普通 Rust case 独立 10 秒 watchdog 也没有被这些来源解决。
- `Q05` 仍是 scripts 测试发现与持续维护入口待定；任务目录 runner 已递归发现，不代表根 test/手动 CI 已接入。依据 `ADR-0028`，也不能为关闭 Q05 将测试、lint 或 smoke 塞回产品发布链。`G27` 仍按既有总账核销；来源没有新的 tag SHA 证据要求重开。

## 结论

- 三份历史文的 OOP disposition 与 defect lead 分类整体符合最小范围。tooling-04 的 batch context 已在基线落地，sampler cleanup 仍是不同的运行时缺口；tooling-02/03 的资源与期限边界不因抽象层存在而自动满足。
- 未发现 A 股交易语义、金额/股份单位、存档/API 契约变更；无需引入交易所规则主张。`ADR-0017` 与本批一致地区分稳定审计身份和实际交易因果，不能把验证工具的字节比较要求冒充撮合语义。
- 不新增或核销 G/Q。保留 `G61`、`G62`、`G63`、`G39` 与 `Q05` 的当前账面边界；不将历史测试列表写成本轮运行结果。没有运行测试、构建或动态平台验证。
