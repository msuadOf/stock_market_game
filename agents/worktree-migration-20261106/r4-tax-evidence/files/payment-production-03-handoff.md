# 付款生产 03 接收与验证交接

日期：2026-10-29。来源基线 `779a7f913255b3e090c94d4f1a655a28c3913c14`，冻结 patch SHA-256 `6cabdb46eeb5caca7cb6175407c17a64e0cb75803defde58cd62f842d21d0881`。

本片仅补 native civil 现金分红与个人收缴：一份真实 `AdministrativeBaseline`，原候选与原 intent 顺序，连续 cash／tax-only stages，同一个 pending/id、最终 INFO、完整镜像和唯一 Commit 成功尾。送转不推进，税 core 与税率不改，未执行 Cargo。

## 已有实际证据

付款 01..03 已在 tests-only 来源精确首红：session57906／WAIT0／chunk60f4e3，三个实际现金差额为0对200、0对180、0对180；front SHA `29c5308a9ca6c3c89072947fd39477ff118bb858a8bce8ea37b641ccb46c580b`。首红后方实施生产。case04 为实现后的回归保护，只验证实际 prepared 付款与 Paid 后主动拒绝的回滚，不登记历史首红。

02 完整未实施者源码准入见 [独审报告](payment-production-02-independent-review.md)。03 仅追加 case04 的真实 `world_accounts_snapshot` 前后完整相等断言；02 原件保留。最终源码复核只允许进入实际构建与必要 green，不代表已通过付款验收。

## 接收约束

唯一接收树为 `.worktree/development-main-2026-10-29`；来源本树不可整文件覆盖较新主干。`session/administrative_mirror.rs` 按净 hunk 适配新入口，必须保留接收方 causal 字段、采集与固定成功尾安装；`runtime/world.rs` 只加入两个 stage 类型的 crate-private reexport，保留接收方较新 WASM 等内容。其余差异亦按 net patch 接收。接收方须审完整合成 diff；本源审不预签该结果。

## 必要后继验证

精确建议名单复用 [16 case 清单](payment-production-02-green-cases.json)：付款4、旧 civil 失败2、原纯候选4、stage5及旧 flat1。root 已允许与 Tax01..05／stage 必要门同图；唯一测试窗由 integration owner 安排，在统一开发主干的稳定验证镜像构建。只需 engine lib 双 feature，不扩宿主或全树验收。普通每 child 与命令树10000ms，批次共享不超过300000ms；显式 j8／并发池，Cargo增量关闭。

实际 green 前，付款完成状态保持待验证；raw cash 与含 purse 总现金不同、资金不足部分 collection、多方案同日、后组／transport 注入、WASM 与双 INFO 等组合仍不能据此宣称已验。
