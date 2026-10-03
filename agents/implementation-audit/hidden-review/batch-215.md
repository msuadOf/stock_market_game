# 批次 215 独立复核

结论：历史审查材料整体可信，所记录的关键缺陷与 43b1aa5 基线源码相符；未发现因本批记录要求而应直接改动产品代码的遗漏。只读检查；未运行测试、构建或 Git 写操作。

## 核查结果

- `tooling-02.md` 是 tooling-02 审查链的汇总与多轮复核记录。静态 Web 打包函数先向输入目录写 `build-info.json` 和 `LICENSE`，再在输出目录依次创建 ZIP、tar.gz、manifest；过程中失败会留下输入副作用或不完整输出。其报告已把 staging/原子发布标为后续方案。`CdpClient.close()` 仅关闭 socket，不拒绝 `pending` 中请求；停止 Chrome 的 Windows 分支等待 `taskkill` exit，但没有证明目标进程树已退出，POSIX 分支仅发 SIGTERM。报告准确区分现状与建议。
- `tooling-03.md` 的调查结论与批次主题衔接一致：deadline 与进程树清理是工具链生命周期问题，不是交易领域状态。其审查记录明确了 server-build 的 300 秒外层 KILL 可能打断 EXIT trap，以及 Windows `taskkill`/close 确认边界；这些结论可由基线脚本行为印证。它没有把调查建议表述为已完成源码修复。
- `tooling-04.md` 的多轮审查修正并最终厘清了每个 seed 的 raw → receipt → aggregate happens-before 顺序。基线并行运行 seeds 并在失败时保留成功 sibling；因此不能推成矩阵级原子提交。`write:false` 跳过 raw 写入，但报告准确说明现有测试缺少直接断言。性能 harness 的采样 Promise rejection 也未接入 child 终止路径；报告把有界清理作为建议而非现状。
- 调用/消费边界：静态包由发布/手动 Web 构建链消费；市场 UI 报告 runner 管理 CDP client、浏览器和临时 profile；simulation baseline runner 的产物由核验工具读取，escrow harness 作为矩阵性能测量工具调用。已读决策没有要求改变这些工具的交易语义。
- 当前 `docs/open-questions.md` 将 Escrow 阶段和分歧指向已接受 ADR-0017；相关 ADR-0027/0028 规定构建和发布链边界。此次工具审计不触及新的待决产品/交易规则。大 A 语义检查通过：改动材料仅描述构建、测量、文件发布和进程生命周期，没有引入或模糊 A 股规则、资金或股份单位。

## 范围与限制

三份指定文件均从头连续读至 EOF，行数与计划一致，SHA-256 与计划一致。基线工作树 HEAD 为 `43b1aa5`。本文只核销调查与审查材料，没有重跑其所述测试，也不代表缺陷已修复或平台验收已通过。必要性/范围判断通过；未见需将工具问题扩大成 engine 或交易规则改动的依据。
