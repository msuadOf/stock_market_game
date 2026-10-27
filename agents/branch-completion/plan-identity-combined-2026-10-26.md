# Plan identity I-D 组合验收（2026-10-26）

状态：writer 的组合验收已通过，等待未实施 reviewer 的完整终审；本记录不是 I-D 自签或 #122 准入批准。生产源码冻结 `c8e9a4f51dfb9021cf95b6da8f7f52cfbcf20957`，独占树 `.worktree/plan-identity-combined`。未运行 full/all、未 merge main、未 push、未实施 #122。

## 机器入口

根 `.tmp/plan-identity-combined/final-manifest.json` 列出完整 argv/cwd/deadline/exit、源码及产物 SHA、原件路径、限制。完整最终 patch 是同目录 `combined-final-complete.patch`。原件保留在根 `.tmp/plan-identity-combined/`；工作 helper 在根 `agents/branch-completion/plan-identity-combined-*`。

## 逐项验收映射

1. **唯一组合及精确移植**：固定 main `c0138972c687a197164fca81d56a9bdc116b4517`。核心12提交 `d6c3ff43 d3a808d0 b9056c4e bab25f82 21cbee78 e8a3d314 83516718 489de2ae c5f1bb20 07349845 5237591a 90e9b624`；runtime仅 `d0c8583c b41c8fb2`（不重复 ae0cae97）；Web9提交 `259f216b b73a5e51 283afef0 68f422f3 c06bf728 f940b81d ed589dc5 f89f5640 f364e64c`。跳过0cf0abb3的三个generated blob与21cbee78完全相同，依据 `plan-identity-combined-generated-dedup.json`。随后 #120 `1afece4c→c04fc956`、D1a `1e54f18b→0a12413d` / `3b535043→1579b8b3`；最后 Monthly `254b092d→c22e547e` / `f84476e4→059647f8` / `8e4e3e91→c8e9a4f5`。
2. **冲突与冻结**：唯一显式 world.rs 冲突按未实施 reviewer 的精确批准合成，保留 main 完整行情预检，再保留 runtime 完整 roster/books/child 预检，再 library_seed/spawn；保 thread_facade::Builder。未整文件覆盖。冲突原件、retention、#120/D1a exact diff 保留；独审批准见 `plan-identity-runtime-review-2026-10-26.md` 末尾。最终根 tree `93edf601a8f1232b5cca376815413c746e8a8e8a`，Web tree `a1de954719c5caa214e4e64f9c2f21bb11c9d8ca`。
3. **实际构建绑定**：Rust 源为1579b8b3；1739输入 digest `6c74fe4115e2575fcd666197ce7a96fa696eae13314c64a18574bf9282f1a634`，构建前后相等。严格 reflink/COW target，未实体 fallback。五 binary、normal rlib、standalone producer 在共享300000ms外监督下构建封存，Cargo jobs8，producer codegen units16，hash threads5，共85.414s。实际 Cargo artifacts、采样、命令和 SHA 在 `build-*`、`cargo-artifacts.jsonl`。最终1741输入 digest `41004d4adb5ee57e20cf22b9e75c07e0ce54dd37f84da660682b837520f808bb`；与1579仅四个 Web 文件不同，所有 Rust 输入不变，Monthly追加后无需重编 Rust。
4. **组合 Rust 短门**：实际新五 binary 的最终有收据复跑：身份17、purse7、严格core identity8（跳过binding export）、behavior1、allocation28、save restore6均绿。关联38命令183case全部绿，含 chain71九精确片恰好一次、plan_owner16、continuation14、knife5b2、atomic4、owner-state1、group roster6、day-end35、admin25及9个精确负控/边界。另真实 world roster消费者8也通过。unit合计215次、210个不同case，跨门存在重叠，不合报成互异总数。每命令10000ms进程树监督，4或8 harness threads，最多12条关联命令同时运行；最长关联命令6.115s。case清单及精确argv见 `final-base-receipt.json` / `final-rust-receipt.json` / `final-extra-receipt.json`。身份17覆盖I-C列出的组路由、policy/terminal/cursor、坏seed、自然日恢复、部分child、失败tick和真实FIFO；saved_runtime精确负控另列于inventory。
5. **最终 Web 同源门**：c8e实际 app/node/DEV 三项目并行 tsc 全绿，共享300000ms外门，app10.035s、node1.587s、DEV2.000s；DEV使用现有 tsconfig.inspector.json。受影响 lint 全绿。I-B原35文件加Monthly新文件共36文件222case全绿，4.038s，Node case与整命令均10000ms、file concurrency8。Monthly原19case3文件包含于这36文件中。复用同一锁与同依赖的ACL cache，未宣称Tauri构建；未改依赖或锁。原件 `final-types-child-receipt.json` / `final-web-receipt.json` / `web-files.json` / `web-cache.json`。
6. **组合真实双向全档**：v1（inst2）及v2代表fixture均因自然计划只有一个owner触发强断言失败，原失败stdout/stderr保留；没有手造plan或弱化断言。root批准inst8代表fixture后，v3由真实GameSession生产API运行两组、两个成功自然日日终，产生day2/tick2、7plan/7owner、各next_sequence1且execution为空。原完整JSON1843635 bytes，SHA `f7babf17a9b52ab61ef7566b64d7a753edf50bc3366cb63e25b592fe25b3c321`；最终Web全对象parse/deepEqual/stringify/reparse/deepEqual通过，回写1843625 bytes，SHA `e7569f707c77aa851b995420facd8d7bd385f126a91ff94d344f8576c5066948`；同组合Rust恢复精确回写档成功。10 bytes差异为JSON number词法重写，完整对象相等。已新增有收据重放：生产2.352s、Web0.569s、Rust恢复1.544s，均10000ms外门；回写与原Web回写字节相同。不同并发生产重放不宣称原JSON字节确定性。v3 binary SHA `f055a389f50e723d5ed08f256ad8354e32fc23f0fc0502ffa11745510e0bfc77`。v2/v3 standalone rustc当时只有stdout/stderr与hash，无集中argv/时序原收据；manifest明确为历史元数据缺项，不伪造原件。
7. **独立终审**：交 `/root/sol61_pending122_contract` 检查完整diff、源码/产物/原件，回答 A股语义、必要范围、边界和跨层一致性三问。有效发现须修复后复审；未经其可信批准不宣称I-D完成。
8. **后续边界**：I-D只证明完整身份前置；Simple/月报真实生产旅程仍未证明。Monthly只增加parser回归，不能替代B4f。#122三档、消费点flush与性能准入须root在可信I-D裁决后另开；EveryTick不翻转。

本批仅身份组合和已批main增量，没有新增A股交易制度或改变100股/元/分等领域单位；资金权威仍为cash+purse_held，child票据不是资金权威。原A股适用依据沿各组件已批正式规则与ADR0043。
