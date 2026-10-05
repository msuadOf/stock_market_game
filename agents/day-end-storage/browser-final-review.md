# Browser IndexedDB 存档批次独立复核

审查日期：2026-10-05 至 2026-10-06  
审查范围：`agents/day-end-storage/browser.md` 所列 Browser IndexedDB 存档 hunk 及相关跨层接线，并包括 root 后续授权的 UI 合并生命周期 group：`usePausePreferences.ts`、`useSaveCommands.ts`、`useSessionHostLifecycle.ts`、相关命令／生命周期／Worker ownership 测试、`DayEndPersistence`、授权 synthetic fixture 两个新增空事实，以及旧单槽 IndexedDB adapter 与专属 suite 退休。只读复核，未修改实现、未操作 Git index、未运行 Cargo 或测试命令。

## 依据与结论

- 已阅读根 `AGENTS.md`、`docs/principles.md`、ADR-0033 与本批次 `browser.md`。ADR-0033 将浏览器本地 WASM 模拟与 IndexedDB 日终存档配对；Remote 由远端 Engine／SQLite 持久化，浏览器不得代存其经济进度。
- 未发现本范围改动引入 A 股交易规则或改变 `Money`／持仓数量单位。存档仍经当前严格 SaveSlot schema 校验：金额为规范十进制分，股份数量为股；日终候选以 `settled_through`、seq 和 settled date 校验，日内活动委托及待处理请求被拒绝。
- 生命周期组只改存档生命周期、宿主控制和测试 fixture，没有变更交易撮合、自然日日结、`Money` 分／股份股、T+1 或费用语义。新增 `retained_market_history: []` 与 `runtime_state.active_minute_history: {}` 仅用于 synthetic `currentSaveFixture`，未声称 Rust restore 有效；历史边界验证拒绝缺失、伪造或不连续事实，不补造历史。
- 未发现阻断合并的有效问题。实现满足仅成功自然日日结自动写档、元数据列表不加载市场、旧代写入隔离、选择元数据失败后不冒称经济市场回滚、删除已选槽不暗选其他档，以及 Remote 不写浏览器 IndexedDB 的范围要求。

## 复核要点

- `IndexedDbSaveRepository` 使用当前三个 ObjectStore，拒绝非当前数据库结构；经济档与 metadata、当前 selection 在成功日终写入事务内一致提交，失败事务不会先发布成功。`save` 只接受 `validateDayEndArchive(parseSaveSlot(...))` 通过的候选。
- `load()` 默认读取 selection 指向的档并交叉核验档案日期／tick 与独立 metadata；显式读档只读取被指定档，不隐式更改当前写入目标。`useSaveCommands` 在 Engine restore 成功并同步内存 UI 后才调用 `select`。选择写失败单独报告“市场已加载”，继续保留所选档作为下一次日终内存写入目标，不执行伪回滚。
- `newSlot`／选择 epoch 与 `DayEndPersistence` generation 配合，旧会话候选不能覆盖新局目标。删除当前被选择档会清除数据库 selection；下次默认读取返回空，不会 load 第一条剩余记录；新的日终保存才建立新槽。
- `useSessionHostLifecycle` 仅 WASM 分支读取浏览器档；WorkerHost 动态导入也只在本地 WASM 创建路径。Native 和 Remote 的日终路径在没有明确文件目标时不请求候选；Remote capability 为 `remote`，App 不添加 IndexedDB 写目标。Native／Remote archiveStore 走宿主 RPC／REST。
- 元数据操作组件调用 `list`／`rename`／`copy`／`delete` 不触发 load；读档只绑定明确的槽 ID。操作错误显式显示，不把失败变成空列表或成功提示。
- `host.load(slot, archiveSlotId?, onRestored?)` 将显式数据库槽 ID、文件导入和 Engine restore 提交确认分开。宿主在 restore baseline 安装后调用 callback；命令层幂等安装 setup 与市场时间参数，因此提交后恢复运行失败不会被描述为市场未恢复。
- `beforeRead` 在文件选择／授权成功后等待调用前已提交的日终写入；用户取消不触发屏障，屏障失败会阻止读旧档冒充成功。快速槽和显式 DB 槽读取也等待屏障。
- Native／Remote 不读客户端 IndexedDB；Remote 生命周期不调用 `stop`，也不以本地 speed／pause preference 覆盖共享市场。WASM Worker 保持懒加载，threadCount 只在受控 E2E 固定为 2。旧单槽 LocalStorage fallback adapter 与专属 suite 退休，当前多槽 IndexedDB 是生产数据库 owner。

## 验证边界

初次存档范围复核没有重跑测试。生命周期融合复核亲自核读 `browser.md` 所列 `merge-lifecycle-second.log` 46/46、`merge-independent-short.log` 18/18、`merge-history-boundaries.log` 15/15，以及真实 Browser connection 红／绿 2/2 和 merge lint 记录；日志所述 timeout 与并发边界符合项目要求。`merge-typecheck-first.log` 有其他 owner 文件及缺失生成类型报错，不计为全项目 typecheck 通过。融合后早期 Browser 的其余四个 case 未重新验收；真实 Engine fixture、复杂 E2E、完整回归仍未验收。退休 suite 的有效需求有当前替代映射，但不等于逐项同实现复测。

上述短测与 Browser 日志不替代 Rust restore 有效性、9.8MB 真实 Engine SaveSlot 或 Native／Remote 全宿主端到端验收；这些仍不属于已完成验证。

## 发现

无需作者修复的具体缺陷。结论仅覆盖上述 Browser 存档与授权生命周期融合范围，不覆盖同仓其他作者的完整 UI／host diff，也不作为全仓改动完成声明。
