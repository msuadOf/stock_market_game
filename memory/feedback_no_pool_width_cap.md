---
name: no-pool-width-cap
description: 用户否决池宽创建时封顶——128 配置=真实 128 线程池，不许按负载缩减池宽
metadata:
  node_type: memory
  type: feedback
  originSessionId: f28cd04a-805e-42cf-b943-c0b6a1ae0d82
  modified: 2026-10-14T07:42:10.849Z
---

用户明确指令（2026-10-14）：不允许池宽创建时封顶。128w 配置下池就是 128 线程，不许按负载缩到 16~32。

**Why:** 用户之前定的部署合同"config 128=自适应上限"指的是任务级准入（活跃任务数=取用），不是池本身的宽度收缩。池宽封顶=改变了"128 线程"的语义。

**How to apply:**
1. 128w 宽度税治理止步于 A+B（任务粗化+scatter-gather）：唤醒 −83%、切换 −53%、128w CPU −15%。
2. 剩余 561ms 税底=rayon ROUNDS_UNTIL_SLEEPY=32 硬编码，如实登记为我方代码外的调度器限制。
3. 不再提出池宽封顶方案（选项①已否决）；vendored rayon-core 睡眠补丁（选项②）未经用户批准也不做。
