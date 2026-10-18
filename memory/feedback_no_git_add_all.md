---
name: no-git-add-all
description: 提交一律显式 pathspec，禁止 git add -A/.——工作区常驻外来未跟踪文件（main 领先内容），范围失控会反复夹带
metadata:
  type: feedback
---

在该项目任何 worktree 提交时，`git add` / `git commit` 必须用显式文件路径，禁止 `git add -A` / `git add .` / 无 pathspec 提交。

**Why:** 2026-10-07 两次事故同根因：工作区常驻大量外来未跟踪文件（main 分支领先内容，如 native-store 包、income_tax.rs 等），E8 的 2b50c6b2 夹带 ~2400 行、42ac1d9b 夹带 37 文件 +12540 行，全靠独立复核事后抓出再清理（备份在 .tmp/e8/foreign-committed/）。

**How to apply:** 每次 subagent brief 的 git 纪律一节必须含此条；提交前 `git status --short` 核对暂存集与预期文件清单一致；发现不明新增文件先隔离备份再移除，不静默删除（可能是用户团队的在途工作）。相关：[[engine-perf-1000x]]
