---
name: worktree-exclusivity
description: 主工作区同一时间只允许一个实现 agent；并行试验/研究 agent 必须在自己的一次性 worktree 内完成一切 git 操作，绝不进主工作区执行还原类命令
metadata:
  type: feedback
---

主实现工作区（.worktree/engine-max-performance）同一时间只允许一个实现 agent 在途；其他并行 agent 的一切 git/文件操作都留在各自一次性 worktree，往主工作区只允许"写 .tmp 产物文件"这一种动作，禁止在主工作区运行任何 git 还原/清扫/格式化命令。

**Why:** 2026-10-07 事故：四路并行化试验与拍卖实现同跑，某个试验 agent 在主工作区执行"按 HEAD 恢复无关文件"，把拍卖 agent 的全部 tracked 在途编辑（Cut A 实现+Cut B 半成品）还原清零。幸亏补丁/设计档以未跟踪文件幸存才可重放，否则整轮工作报废。

**How to apply:** 派多个 agent 时：实现类任务与主工作区一一独占；试验/研究类任务的 brief 必须写明"你的 cwd 就是一次性 worktree，所有 git 命令只在那里跑；往主工作区交付产物只用文件写入（.tmp/），永不在主工作区执行 git 命令"。实现 agent 的在途未提交编辑是最脆弱资产，任何并行者不得触碰其工作区。相关：[[engine-perf-1000x]] [[no-git-add-all]]
