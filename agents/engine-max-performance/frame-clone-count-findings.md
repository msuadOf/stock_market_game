# Intraday TickFrame clone 计数实验（已撤回）

假设：`ProtocolSession::step_frame` 旧代码 `intraday.push(Arc::new(frame.clone()))` 会在返回 owned frame 之外，为 append-only history 额外完整克隆一次 `TickFrame`。候选把 frame 先放入一个 `Arc`，历史保存 `Arc::clone`，公开返回值仍保留原有 owned API，因此理论上只保留返回值这一次克隆。

实现范围：仅 `packages/engine/src/session/protocol/civil/session.rs` 的 `step_frame` 与 `step_frame_with_commit_evidence`；11 行 diff，完整 patch 见 `frame-clone-count-experiment.patch`。无交易语义、门禁或协议字段变化。

构建与测试：
- debug `-j8` exit0（5.7s），release 普通 production probe `-j8` exit0（32.9s），外部 299s deadline。
- verify-build 通过，engine/server features 为空，binary SHA `c9b1526aa40fb9f402e8faa86a2ca19bd621d2c8f9afb0b243c8b51ecebd0ff8`。
- protocol 相关 47 个测试通过（`protocol` 宽 filter，含 rollback / publication / facts / bindings），9s TERM + 1s KILL，RAYON4/test4。

四条基准顺序独占执行，默认 5 股 / 20,007 NPC / seed 42、16 物理核 `0-15`、Rayon16/Tokio2、3000 tick：

| 样本 | SHA 前12 | speed_x | wall_s |
|---|---:|---:|---:|
| frame-clone-count-16 | c9b1526aa40f | 223.040 | 13.455 |
| frame-clone-count-confirm-16 | c9b1526aa40f | 217.658 | 13.792 |
| frame-clone-count-base-16 | eac5e6f2fa22 | 220.021 | 13.635 |
| frame-clone-count-base-confirm-16 | eac5e6f2fa22 | 223.969 | 13.404 |

candidate 均值 220.349x，相邻已提交 382c1871 均值 221.995x，差异约 -0.74%，完全在现有波动内；没有证据支持该额外 clone 是当前瓶颈。源码已逐字节回退到 HEAD，主 release/test artifact 不再代表当前源码；不独立复核、不提交。
