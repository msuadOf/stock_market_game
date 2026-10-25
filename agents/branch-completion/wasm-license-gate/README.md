# W0L：wasm-pack LICENSE 构建门

日期：2026-10-25。基线：`main@f5820291`。分支：`fix/wasm-license-gate`。

## 范围与依据

本批仅新增 `scripts/wasm-pack-build.mjs`、必要的文件系统测试，并在
`frontend-build.mjs` / `build-targets.mjs` 的实际 wasm-pack 执行处共用该门。
不改 engine、交易规则、存档、API 或 WASM runtime，不改变现行 A 股概念或单位；
不引入依赖，不涉及新的领域规则决策。独立完整 diff 审查由 Leader 另行派发，本记录
本身不代表审查通过。

已用 `gh api` 读取官方 `rustwasm/wasm-pack@v0.13.1`，本机 `wasm-pack --version`
为 `wasm-pack 0.13.1`。官方实现：

- [src/command/build.rs:221](https://github.com/rustwasm/wasm-pack/blob/v0.13.1/src/command/build.rs#L221)：
  `crate_path.join(PathBuf::from(build_opts.out_dir)).clean()`；Git blob SHA 为
  `9960c96cc6c1d6a4d3e545583f04aa538c8092f0`。
- [src/license.rs:73](https://github.com/rustwasm/wasm-pack/blob/v0.13.1/src/license.rs#L73)：
  源为 `path.join(&license_file)`，目标为 `out_dir.join(&license_file)`，随后调用 `fs::copy`；
  Git blob SHA 为 `5305e41e378f61f6bfff94b5828ae8eca18adec8`。

因此 Cargo 的 `license-file = "../../LICENSE"` 不会自然生成 `pkg/LICENSE`。
helper 固定并核对本仓库这条 manifest 契约；构建前后核对源非空及 SHA256，不同即失败；
执行前用 realpath 与 dev/ino 拒绝源和 wasm-pack 目标或实际 package LICENSE 的同文件别名；
执行后使用当前源 inode 再核对，显式 copy 到实际 `pkg/LICENSE` 并校验内容。
未实施者预审发现非源 symlink/hardlink 可能覆盖无关文件，已补红证并收紧同一目标校验：
两个输出目标只接受不存在的路径，或 nlink=1 的普通文件；symlink（包括悬空）和共享
hardlink 均显式拒绝，构建后再次检查，保留无关文件原字节。
相对 out-dir 按 crate 解析；目录 symlink 的 `../..` 按文件系统真实目录解析。
构建失败仍复核源，双错以 AggregateError 保留，失败不补发 package LICENSE。

## TDD 与验证原件

- `red.txt`：helper 不存在时，同一组最初的行为测试失败。
- `red-replaced-inode.txt`：相同内容替换源 inode 后，旧实现漏拒绝 hardlink，断言失败；
  修复为使用构建后的当前源身份。
- `red-external-link.txt`：独立审查发现非源 symlink/hardlink 会被覆盖后，新断言先失败。
- `green-helper-frontend.txt`：42/42，约 2.1 秒，涵盖 helper 与 frontend。
- `green-native-a.txt`：11/11，约 1.5 秒。
- `green-native-b.txt`：7/7，约 5.1 秒。
- `green-native-c.txt`：5/5，约 1.2 秒。

四条最终普通命令均经 `node scripts/run-with-deadline.mjs 10000 --` 进程树门，
Node 同时使用 `--test-timeout=10000`。helper/frontend 用 `--test-concurrency=4`；
native 三片互不重叠、同时运行，完整覆盖该文件的 23 个测试。
所有新增 fixture 位于该 worktree 的 `.tmp/w0l-*`，结束后清理；已有测试的路径不扩大改动。

最终命令：

```sh
node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=4 scripts/wasm-pack-build.test.mjs scripts/frontend-build.test.mjs
node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-name-pattern='^(parser|pure|Web plans|Desktop uses|dry-run|artifact publisher|batch|fresh frontend|repeat Web|shared supervisor|native UI)' scripts/build-targets.test.mjs
node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-name-pattern='^(shell Server|supervisor kills|native target|publisher cleans|production compiler|failed Cargo|native bundles)' scripts/build-targets.test.mjs
node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-name-pattern='^(prebuilt UI|Linux portable|desktop compile)' scripts/build-targets.test.mjs
```

构建源 `LICENSE` 仍为 1062 字节、SHA256：
`e57d1f88b21c0b41859a0c884768d168a16bb9924c4681b3eddc6b83b32ad3c8`。
`git diff --check` 通过。

## 诚实限定

本批未运行真实 wasm-pack 编译，文件系统门的行为均由代表性短 fixture 验证；
不把 fixture、官方源码或旧构建记录冒充新的产品构建证据。W0 的真实构建仍须独立执行。

第一次把 build-targets 整文件与其他测试放进一个 10 秒批次时达到 deadline，已保留
`green-integration.log`（文件名是当时预期，实际日志为超时，不是通过）。
初始工作树路径含 `wasm-license-gate`，既有 pure Server 测试对整个 plan JSON 的
`/wasm/` 断言误匹配 cwd；保留 `build-targets-focused.log`，将工作树移至
`.worktree/w0l-gate` 后分片通过，未弱化断言。`red-integration.log` 也包含该路径误匹配，
仅作历史失败原件，不作为最终门禁结果。上述辅助日志留在本工作树，最终提交保存红绿主证据。
