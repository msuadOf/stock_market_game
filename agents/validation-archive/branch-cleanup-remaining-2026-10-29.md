# 未挂 worktree 的已合并分支集中清理（2026-10-29）

## 范围与门禁

续接用户“及时清理已经合并、废弃的分支”授权，只处理未挂 worktree 且 HEAD 为 main 祖先的本地 ref。下表是删除前固定选集，共 68 个；不移除工作树、不清理缓存、不删除未合并或独有产品 commit、不 push、不 prune。main / 根 HEAD 和既有 dirty / index 不变。本批不改变产品代码、A 股规则或正式文档，无需产品测试。

删除前 main `11cb97fa336c486b5e30526afce17b3e87e51533` clean；根 `af0fdeb6c952f294e3069f031c38f4d8ad0fd6b4` 保留原 4 项 tracked 修改，index 为空。所有已挂树均排除，包含 dirty/WIP、应用管理树、活跃任务和共享 runner 路径。

仅在 owner / runner 按 ref 消费核对以及未实施者独立审查通过后，从 clean main worktree 执行 `git branch -d -- <branch>`；任何拒绝均保留，禁止 `-D`。历史文档提及分支名不自动视为活动消费者；实际脚本消费者和冻结锚证据不足者保留。

## 固定选集

每行由 `git for-each-ref --merged=refs/heads/main` 取得；删除前重验精确 OID、main 祖先关系、无 worktree。

| branch | OID | 结果 |
|---|---|---|
| `archive/rights-r3b-takeover-d5ac-2026-10-27` | `d5acde4de855c4151514233867aecd0c159f8ada` | 已删除（-d exit 0） |
| `archive/source-b-eea-green-2026-10-27` | `eea08824f311010db8c8673541cbb3b358059cb6` | 已删除（-d exit 0） |
| `evidence/tax-admin-pre-source-2026-10-27` | `98d509a31415dffd78a6cd2112b081648e413bbc` | 已删除（-d exit 0） |
| `fix/engine-repurchase-registry` | `0be381615b984ff843133490d8cbd4a7a288947f` | 已删除（-d exit 0） |
| `fix/engine-repurchase-share-units` | `b2aa7fe1f98ae10ad225903d5e154b59132be0a2` | 已删除（-d exit 0） |
| `fix/main-release-validation` | `b76ece39b3a1635adde52da07375607f19b56ecc` | 已删除（-d exit 0） |
| `fix/spine-simplify-01` | `55b898c72ae37892e8f621fa2fe7cc84278c1ab9` | 已删除（-d exit 0） |
| `fix/synthetic-history-policy` | `b89afb3346743a4b4fccf26c9ac9ff108595f696` | 保留（-d 拒绝，exit 1） |
| `fix/web-quality-closeout` | `ea7cfac99ca13fa57470e9e931606abbdff32960` | 已删除（-d exit 0） |
| `fix/web-worker-error-transport` | `b4c51f43cdfb8d661d57240ebd1faa5786260e39` | 已删除（-d exit 0） |
| `frozen/perf-baseline-5412fd10` | `5412fd10a9901613929f45ca96ab6d36a947021a` | 已删除（-d exit 0） |
| `frozen/pristine-check` | `3b9e091ec19694e6f49a3b3c0f3c755f29837a3f` | 已删除（-d exit 0） |
| `integrate/d2b-e2-diagnostics-2026-10-27` | `1a7770e2c401a5b71106b5eda5c6596ddb0146e5` | 已删除（-d exit 0） |
| `integrate/d3-p1-2026-10-28` | `9c82c9c56becc1cc196f5792970146e5471d2ee1` | 已删除（-d exit 0） |
| `integrate/d3-p1-disc-tax-2026-10-28` | `cd6e88c2aa3f9247049a2c912ed42c708f605a70` | 已删除（-d exit 0） |
| `integrate/d3-p1-frozen-2026-10-28` | `9c82c9c56becc1cc196f5792970146e5471d2ee1` | 已删除（-d exit 0） |
| `integrate/info-runtime-staging-2026-10-27` | `1a7770e2c401a5b71106b5eda5c6596ddb0146e5` | 已删除（-d exit 0） |
| `integrate/native-union-2026-10-28` | `db112f3a1dd31a954af4b64b72b393f56acc8619` | 已删除（-d exit 0） |
| `integrate/origin-query-f5-2026-10-28` | `d09c8db6e73a975f378866a9d4fcc6a5c848ebe1` | 已删除（-d exit 0） |
| `integration/correction-parser-leaf-2026-10-27` | `9e7c3da2c41f89930d186e3061c11792fb60780a` | 已删除（-d exit 0） |
| `integration/proposal-parser-leaf-2026-10-27` | `d62041833d02bde8f7032b449bbed607b23a4969` | 已删除（-d exit 0） |
| `perf/engine-owned-p5-ledger` | `f3c64bc059cc15b852b8884ee2e07d890bd91101` | 已删除（-d exit 0） |
| `perf/engine-p4-projection-ownership` | `96692b1a988350b04682fd139408c3f1b6533f2d` | 已删除（-d exit 0） |
| `perf/engine-parallel-stock-boundary-20260926` | `96ed302efbec62d78c20c0afcab0604c2348f75f` | 已删除（-d exit 0） |
| `perf/engine-private-stock-finish-audit` | `926c1dd6103bb7cb71687a5a85c00ffb6e708bee` | 已删除（-d exit 0） |
| `perf/engine-private-stock-round-20260926` | `4188a23f1aa4ed3fed36777223534f2275e92fad` | 已删除（-d exit 0） |
| `perf/engine-sparse-continuous-projection` | `d5bc38e2d21b52fde76b4d3125fb371cdc9565a5` | 已删除（-d exit 0） |
| `perf/engine-sparse-fill-receipts-20260926` | `6423b31ac215acb66248fd28b7ede7b00f15054c` | 已删除（-d exit 0） |
| `refactor/oop-complete` | `ebfb68bb7bd37bdff79d904faceb4abe04783efb` | 已删除（-d exit 0） |
| `refactor/responsibility-names` | `b5b846421abc762e09332d8a7bd479f72d30a855` | 已删除（-d exit 0） |
| `test/cleanup-tooling-and-isolation` | `7369cc960de742308cf56b803238d9d277c01fcd` | 已删除（-d exit 0） |
| `test/clear-baseline-failure-fixtures` | `3b9e091ec19694e6f49a3b3c0f3c755f29837a3f` | 已删除（-d exit 0） |
| `test/migrate-test-contracts` | `646742306f0cb854d47cda3ef3299a5d7a0efe80` | 已删除（-d exit 0） |
| `test/web-wasm-acceptance-guard` | `00a68135ea2696fce07658e8b2d85a7734ffe3b6` | 已删除（-d exit 0） |
| `verify/red-baseline` | `3056d0d77d271169d9436d884911dcc4c328208e` | 已删除（-d exit 0） |
| `worktree-agent-a06d9ddfb23cddf1f` | `c2accc881862091ace646623929b060fc0478fb2` | 已删除（-d exit 0） |
| `worktree-agent-a0a649ef01f1c318d` | `ff26a4e4ec66e96b1d3fbadf187410dc242e73cc` | 已删除（-d exit 0） |
| `worktree-agent-a0cf2217697ea6033` | `c2accc881862091ace646623929b060fc0478fb2` | 已删除（-d exit 0） |
| `worktree-agent-a0ddd092830f05e79` | `ec7a46cd5d11a50fc131a59642e72f0fb1592eb6` | 已删除（-d exit 0） |
| `worktree-agent-a1c9f0dbed644a1d9` | `e60ac02c6f6925b0cf582b299a8f25559f14e49d` | 已删除（-d exit 0） |
| `worktree-agent-a1fb819e0b002019c` | `609ca07ed3c26e678e2e4bb90650f4ed1e196190` | 已删除（-d exit 0） |
| `worktree-agent-a230a0f4128fe9f7a` | `c1561faf7aaf0caf4113489ae81566566b3ceb96` | 已删除（-d exit 0） |
| `worktree-agent-a2744004a49a2f0e7` | `ff26a4e4ec66e96b1d3fbadf187410dc242e73cc` | 已删除（-d exit 0） |
| `worktree-agent-a34848a39507c5176` | `b420caf1fa866225d1e0d90a007d6d139d16d50a` | 已删除（-d exit 0） |
| `worktree-agent-a3ed2e85eee882afd` | `92a052971a69bff956d291780225eabb9847b41d` | 已删除（-d exit 0） |
| `worktree-agent-a448e00f09d0446c9` | `5a940264d208360e073d6b213cc09895a48ee3ad` | 已删除（-d exit 0） |
| `worktree-agent-a5f92437af100ea6d` | `3b9e091ec19694e6f49a3b3c0f3c755f29837a3f` | 已删除（-d exit 0） |
| `worktree-agent-a74541ec8ce72a191` | `765861b5b8418ba788cefb8958c3ece59290768a` | 已删除（-d exit 0） |
| `worktree-agent-a9b38446b5112eb51` | `7aaf6659b6af5a16364ea67ba194fd83cc5a5fd8` | 已删除（-d exit 0） |
| `worktree-agent-ab41856784a78cc9c` | `0be381615b984ff843133490d8cbd4a7a288947f` | 已删除（-d exit 0） |
| `worktree-agent-abb7a96b599183e26` | `ff26a4e4ec66e96b1d3fbadf187410dc242e73cc` | 已删除（-d exit 0） |
| `worktree-agent-ad9597b19c8a67693` | `ff26a4e4ec66e96b1d3fbadf187410dc242e73cc` | 已删除（-d exit 0） |
| `worktree-agent-afc56ec75b2f12651` | `3b9e091ec19694e6f49a3b3c0f3c755f29837a3f` | 已删除（-d exit 0） |
| `worktree-wf_20860509-c0a-1` | `3d5bbd2a28a941ef16ddd5abca625e0f1ae0ba76` | 已删除（-d exit 0） |
| `worktree-wf_295d21c5-185-1` | `d3cb4c7491fb67732c7d4d9f0f234874d3f1e1e2` | 已删除（-d exit 0） |
| `worktree-wf_5835e503-dc5-1` | `dd82b3c39e6f4596c1d25f63882344ed1630dfbc` | 已删除（-d exit 0） |
| `worktree-wf_58e8fbc7-359-1` | `d895ce5cd6f04a3aa85f94889fcda05e631fb800` | 已删除（-d exit 0） |
| `worktree-wf_611ff3c3-839-1` | `ceaeda2b3c94d96e6943f5af794cd3047f99e26f` | 已删除（-d exit 0） |
| `worktree-wf_62c55998-34b-1` | `c2accc881862091ace646623929b060fc0478fb2` | 已删除（-d exit 0） |
| `worktree-wf_722872a1-cae-1` | `af9809c701f6d2f40556d98a3912be00f95f5704` | 已删除（-d exit 0） |
| `worktree-wf_7c22f441-f6a-1` | `4f12a6f5a097450b3bdaca5ad8a809cb04f84123` | 已删除（-d exit 0） |
| `worktree-wf_7c22f441-f6a-2` | `4f12a6f5a097450b3bdaca5ad8a809cb04f84123` | 已删除（-d exit 0） |
| `worktree-wf_7d5713c4-666-1` | `619ec66e84302596e3aa85c41e3f4d1b52f0771f` | 已删除（-d exit 0） |
| `worktree-wf_95bfe91e-44b-1` | `e5b3304dcad11370641104456c3905ca032bf4f5` | 已删除（-d exit 0） |
| `worktree-wf_a9ae480c-bdf-1` | `9b8f9f391dad309e04e0789644b1ef802ddabf3a` | 已删除（-d exit 0） |
| `worktree-wf_eb0a457b-037-1` | `6579e64106ea6bb698568d8b80f9468da2dde089` | 已删除（-d exit 0） |
| `worktree-wf_fc2c5f31-d83-2` | `a30a845dfe39dcec8dca61a7c2ee3b7f1b22e96e` | 已删除（-d exit 0） |
| `worktree-wf_ff9d1264-f14-2` | `4f12a6f5a097450b3bdaca5ad8a809cb04f84123` | 已删除（-d exit 0） |

## 集中核对与执行结果

- owner / runner 依赖：root 确认重点 9 个 archive / integrate ref 无当前 owner，并检查当前 E2green / Host / restoregreen02 / 共同 runner 源码，git 调用只取各目录 HEAD 或显式 OID，未发现按旧 ref 取 source。Host owner 另精确检查 E2green23 / Hostjoint02 全部冻结 runner / plan 小文件，9 个重点 ref 0 引用，实际只有 helper 仓库 `rev-parse HEAD` 后对冻结 OID；确认删除无影响。integrate owner 定点核对 D3/P1 旧批、expanded、product/native/typed-info 与现 four-red 全部文件，只在两份历史 README 提旧 ref；当前及待执行共同四红 / E2-Stock 候选均按固定 OID 和 HEAD / HEAD^{tree} 校验，不以所列 9 ref 作 git argv，确认均为历史别名。当前 `integrate/engine-main11cb-red-2026-10-28` 已挂树，保持排除。
- 执行者对根 agents 和 5 个活树及 runtime-sync-e1 / info-runtime-staging 的 agents 下 py / sh / mjs / cjs（单文件至多 2MB）检索全 68 名，0 命中；JSON 命中 240 行，归并为 2026-10-25 历史 ref 盘点、funds takeover、runtime-diagnostics composition-source9 历史组合元数据、source-b before、oop plan。关键 JSON 同时保存固定 OID / 完成事实，未发现实际执行 argv 消费。此证据只覆盖上述明确范围，不声称全机扫描。
- 未实施者独立复核初核：同一未实施 `gpt-6.1-sol high` 已核实 68/68 精确 OID、main 祖先、未挂树和保护基线；所有本地 heads 没有以 68 项作 local upstream 的消费者；agents 外 tracked py / sh / mjs / cjs / js / ts / yml / yaml / toml / Makefile 检索完整分支名均 0 命中。owner 闭包确认后已获删除前 `APPROVED`。
- 预期保留边界：`fix/synthetic-history-policy` 配置的 upstream `refs/remotes/origin/fix/synthetic-history-policy` 存在，但本地 OID 对 upstream 的祖先检查 exit 1。即使 main 可达性成立，`branch -d` 仍可能拒绝；保持 upstream 原配置，不绕过、不 `-D`，按真实结果保留。
- 实际执行：68 项逐项执行 `branch -d`，67 项 exit 0 已删除，1 项被 Git 拒绝并保留；未修改 upstream 配置或升级强制删除。执行后 67 个 ref 均不存在，原 68 个 OID 均仍由 main 可达；worktree 登记逐字相同；main / 根 HEAD、main clean、根原 4 项 tracked dirty 和空 index 均不变。未实施同一 `gpt-6.1-sol high` 删除后独立复核 `APPROVED`：67 ref 确已删除，唯一保留项的原 OID / upstream 未变，68 原 OID 仍由 main 可达；main / 根身份、根 4 项 dirty、空 index 和重点共享保护树身份一致，无新增发现。全 worktree 登记逐字相同仅采用执行者删除脚本运行时保存前值并 assert 相同的原始证据，reviewer 未独立重演全量比较。当前剩余未挂树的 main 祖先分支仅为该保留项。

Git 拒绝原文：

```text
fix/synthetic-history-policy
warning: not deleting branch 'fix/synthetic-history-policy' that is not yet merged to
         'refs/remotes/origin/fix/synthetic-history-policy', even though it is merged to HEAD
error: the branch 'fix/synthetic-history-policy' is not fully merged
hint: If you are sure you want to delete it, run 'git branch -D fix/synthetic-history-policy'
hint: Disable this message with "git config set advice.forceDeleteBranch false"
```
