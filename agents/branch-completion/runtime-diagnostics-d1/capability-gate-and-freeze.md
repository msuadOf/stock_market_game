# D1 capability gate 复核修复与冻结

- 冻结源码：`build-2-source-before.json`，覆盖当前候选完整 engine / Cargo 配置；feature 与 default 两次构建结束后逐项 SHA 一致。
- 独立预审唯一实际阻断：未接通的 Market 余量、真实 Filled、Expired、非连续申报与 DayEnd 终止可能只留下 Submitted，报告会误写 open。旧候选新 Market/DayEnd 两测试真实失败，输出 `Ok` 且含实际 `OrderCanceled`，没有伪造诊断能力错误。
- 红 binary SHA256：`4b9f8284a928eef98fcafef439c6808f8930d568c302d09aa4fe20bcb3446112`；实际 build-1 engine rlib SHA256：`63ee63f858403328ecda1b9369ddfedc918a0f6a443cf3ca8b52f538a4843376`。rlib 仅严格 reflink 保留在根 `.tmp/runtime-diagnostics-d1/frozen-unsupported-red/`，避免后续 Cargo 覆写丢失红证。红测编译成功耗时 2.73s；先前 10s 编译超时、一次依赖 serde 身份/API 编译错误均不计 TDD 红。
- 修复：真实成功点写 feature-only `Unsupported` raw marker，prepare 显式失败并回滚；不实现或伪造 D2/D4 事实。自然日末与三个手动 Drain 入口共用余单能力门，只读既有发布与真实初始种子存在性，不增加查询往返。初始 v0 的 resting 观察硬编码为空，checkpoint 重建后以初始真实 `initial_desk_orders` 存在性补核，不能用默认 0 绕过能力门。
- 实际 feature 6 binary 构建：`-j8`、独立外部 300000ms，55.52s 成功；有效 89 条资源采样，相关进程累计 CPU 相邻差峰值约24.17核、单进程最多17线程、RSS峰值约3.76GiB。这不是逐线程 runnable 取证，不宣称全机满核。
- 实际 default 2 binary 构建：同 target/profile、`-j8`、独立外部 300000ms，70.37s 成功。两批独立预构建均不是完整 all，不把两次上限拼成一次完整验收。各 supervisor 已 wait、无 Rust/Cargo/linker 残余，重窗口已交其他任务。
- 所有普通测试命令、case 均受 10000ms deadline；并行8。新增内部13项与过滤命中原3项共16/16，公开5/5，collector7/7，feature固定合同1/1，default absence2/2、固定合同及helper负控4/4通过。固定合同核对真实事件、现金、保存恢复，feature off 无新增 sideband/字段/hook。
- 原 integration 已用当前 source 实跑25项：13过12红。旧27清单还包含 company_scale 两例，尚待其受控单 binary 构建；不能将25冒充27。未删除旧16红证，未弱化原断言。
- 原绿转红两项必须单列：`diagnostics::baseline_excludes_preset_history_and_reconciles_trade_volume_to_daily_candles`、`diagnostics::baseline_keeps_each_seed_visible_and_summarizes_observed_runs`，均因本中间切片 `NonPlayerUnresolved` 能力门显错；不能只比较原16红与当前12红来宣称整体改善。六项原红转绿只记录当前组合结果，其中包括serde/算术/ID用例，不全部归因于D1新增实现。
- D0b / D2–D5 尚未完成；D1仅中间切片，不单独合 main。完整 implementation diff 已由未实施 `sol61_branch_task_inventory` 终审 APPROVED，仅最小 D1 中间提交；正式裁决见根 `agents/branch-completion/runtime-diagnostics-d1-review-2026-10-26.md`。27 矩阵尚缺 company_scale 两例不阻塞此代码保存，但仍须按协调窗口补齐，不宣称整项诊断交付完成。
