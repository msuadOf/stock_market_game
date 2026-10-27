# JSON 证据补入暂存区独立复核

日期：2026-10-28。本轮仅审查产品提交 `5270300edc9507ed6fc379d9ce843b6034e657fc` 之后暂存的 JSON 证据，不修改产品、Git ignore 或历史原件，不重跑测试。复核者未实施产品改动，也未实施本次 `git add -f`。

结论：**本次 27 个 JSON 证据文件的补入通过独立复核**。既有产品结论仍限定为 SOURCE_ONLY；本次证据补入不增加真实 Rust producer、宿主编验、browser 或 F5 总成的验收范围。

## 核对结果

- `git diff --cached --name-only` 恰为 27 项，全部位于 `agents/branch-completion/company-web-f5-read-2026-10-28/` 下，以 `.json` 结尾，且均为新增 `100644` 普通文件；暂存区没有产品文件、`.gitignore`、可执行文件或其他主题路径。
- 对全部 27 项直接读取 index blob（`git show :<path>`），全部可按 UTF-8 JSON 解析；index blob 与当前工作树原件逐字节相同。内容仅为 receipt、TypeScript 分片配置、独立 manifest 核对结果和 Worker 静态 apply 检查记录，未发现 ELF、NUL 二进制或凭据。已检查私钥标记及 GitHub/OpenAI/AWS/Bearer 典型凭据形状，也逐份核对其结构与命令参数；配置中的绝对工作路径是既有原始执行证据，不在补证时重写。
- 最终 `issuer-green/receipt.json` 的 index blob SHA-256 仍为 `1c59128e421e13aed02d1659b742774818f04f74598e5be189ae04c708214783`，与第三轮独立审查记录一致。
- 该 receipt 的 14 项产品 SHA-256 均直接对照 `git show 5270300edc9507ed6fc379d9ce843b6034e657fc:<source-path>` 的 commit blob 验证通过；14 项当前工作树也与对应 commit blob 字节一致。没有把工作树查询结果冒称 commit 绑定。
- `static-v2/receipt.json` 的 index blob SHA-256 仍为 `c2cd591c16c91e790275aafe0a1e9a64980dc15ae227b01ea7849fc14422ef71`；`independent-review-v3/manifest-check.json` 的最终 receipt SHA 与上述值一致。
- 对含历史源码 SHA 的 `red-v1`、`red-v2`、`host-red-v1`、`transport-red-v1`，逐项与同目录保留的源码快照核对；两份 Worker compatibility 记录的 `patchSha256` 也与各自保留的 patch 核对。合计 7 项 SHA 全部匹配。两个 compatibility 记录均明确 `onlyStaticCheck=true`，不冒称已向 W1 写入或执行通过。
- `static-v1` 与 `static-v2` 的四个 TypeScript 分片配置均可解析，引用的文件路径均存在；各自合计 764 项、761 个唯一路径，公共 ambient declaration `wasm-pkg.d.ts` 在四分片各出现一次。该重复与每个独立 TypeScript program 所需声明一致，不是产品文件重复加入 index。
- 原 receipt 保留失败 exit、真实红灯、旧 lint 失败和成功结果，没有把补入 JSON 当作重新运行、覆盖旧失败或恢复执行日志。部分历史 receipt 没有全量源码 manifest，本轮只核对其实际具备的绑定，不补造历史证据。

本轮核对不涉及 A 股交易、账户、税率、金额单位或结算逻辑变化；补入这些被全局 ignore 跳过的既有证据，是保持先前复核记录与原件引用完整的必要最小范围。
