# UI merge base 生命周期冲突独立复核

## 范围与结论

本复核限定于本次 merge base 冲突的六个文件：`apps/web/src/app/useSaveCommands.ts`、`apps/web/src/app/useSessionHostLifecycle.ts`、两个对应测试文件、`apps/web/src/host/wasm-worker.ts` 与 `apps/web/src/host/wasm-worker-ownership.test.ts`。逐文件阅读了工作树版本、index stage 2（main）和 stage 3（feature）的完整内容及冲突差异；没有改代码或 index。

对这六文件的 base merge 结论为 **PASS**：两侧独立变更已保留且组合顺序合理，未发现日终写入/读取次序、setup 安装时点、会话 generation 或 Worker restore authority 的回归。未据此宣称 stash 后续 live 集成已完成。

## 复核要点

- **存档与日终屏障：** feature 的快速槽读取在读档前显示等待提示并执行 `beforeRead()`；文件读取和 recover 路径也传递提交屏障。读取完成后校验档案、失效并等待队列、确认 replacement generation，再安装 source 或调用 restore。新局仍使旧日终候选失效并等待写入退出。现有等待、失败可见、替换宿主后旧操作不触碰新队列等断言保留。
- **setup 与开局分配：** main 的 `floatAllocationDraft` 保留在新局 setup，生命周期创建新局后仍查询并显示真实 `initialAllocation`，取消时丢弃晚到结果。feature 的 `configureMarketTiming` 保留在启动档加载后、协议接线前。快速槽和文件读档通过同一幂等 `onRestored` 回调，在宿主确认 restore 已提交后安装 market timing、active setup、日期/价格笼子/float allocation 草稿并清除旧初始分配；提交前失败不提前安装，提交确认后即使后续恢复运行失败也不回滚已提交 setup。测试覆盖了该边界。
- **Worker：** main 的共享 WASM 初始化与 ingress bridge 保留；feature 的 baseline、restore 响应和 civil-date 字段保留。restore 使用新 handle/generation 发布 ingress，再发送旧 generation 的 `restored` 确认和新 generation 的 baseline；restart 延至 microtask，因此消息同步发布完成前不会提前重启。`stockHistory` 仍要求当前 generation。测试保留 ingress、新旧 generation、civilDate、基线顺序、microtask restart，以及 snapshot/prepare 失败的 authority 断言。
- **必要性及语义：** 变更是两侧已授权功能的最小组合。改动不改变 A 股交易撮合或结算规则；读档只恢复档案的 `SessionSetup` 与引擎状态，不以恢复后的持仓伪造开局分配。

## 后续接口风险

当前 feature/base 的 `EngineHost.load(slot, onRestored?)` 把第二参数定义为 restore 提交确认回调，并由 Remote、Tauri、Worker 宿主在提交确认点调用。root 暂存的多槽改动计划占用 `load(slot, archiveSlotId?)` 第二参数。**恢复 stash/live 工作时必须显式消解这个签名冲突**，例如统一成 `load(slot, archiveSlotId, onRestored)` 或具有同等清晰度的命名参数对象；不能把 `onRestored` 函数传成 archive slot ID，也不能移除提交确认回调或把回调提前到提交前。此项是后续 stash/live 接续的集成门禁，不是当前六文件 base merge 的未解决冲突。

## 验证证据

亲读 `.tmp/ui-design-merge/base-lifecycle-short.log`：32 项实际短测全部通过，`pass 32 / fail 0`，记录总时长 325 ms。该日志包含存档命令、宿主生命周期与 Worker ownership 两项测试。没有运行 Cargo 或完整回归；本 review 不扩展为对其它文件或 stash/live 工作的验收。
