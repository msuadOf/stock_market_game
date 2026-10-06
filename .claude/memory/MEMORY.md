# MEMORY.md

- [协作分工模式](feedback_collaboration_mode.md) — 主 agent 只对话协调，开发交给 subagent，允许二级 agent
- [engine 1000x 性能主线](project_engine_perf_1000x.md) — 16核 ~220x→1000x，worktree .worktree/engine-max-performance，台账 current-state.md
- [不用 /tmp](feedback_no_tmp_dir.md) — 产物放工作区 .tmp/（perf 主线：.worktree/engine-max-performance/.tmp/）
- [大白话汇报](feedback_plain_language.md) — 结论要让外行看懂："通过什么方式达成什么结果"，不堆术语
- [不需要确定性](feedback_no_determinism.md) — 并发下不要求可复现，16 线程结果不一致不算 bug
- [无版本概念](feedback_no_versions.md) — 不用 v1/v2，一切可破坏性修改，不考虑兼容性
