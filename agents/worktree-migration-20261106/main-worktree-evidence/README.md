# 历史工作证据归档

本归档来自本机 `.worktree/main-merge-2026-10-25/agents/` 的两组工作文件，保留其目录和文件相对关系。原始材料仍在本机，归档不是重新执行或验证。历史记录中的性能数据和测试结果仅代表当时记录；尤其 `wait-boundaries/实施记录.md` 中 Cargo 验证超时且清理门禁未确认，不能视为成功。

为保护本机路径，文本中的仓库根目录统一替换为 `<WORKSPACE>`，用户目录替换为 `<HOME>`，临时目录替换为 `<TMP>`；此外，`implementation.diff` 的一处空白行和 `direct-tests.log` 末尾的多余空行已规范化；其余内容保留。下表的 SHA-256 对应脱敏副本，而非原件。`wasm_wait_primitives-direct` 是可再生的约 6.9 MiB 构建产物，未纳入；空日志仍保留。

| 脱敏副本（相对于本目录） | SHA-256 |
| --- | --- |
| `perf-remeasure-20261103/README.md` | `e0ec2aea677f1a4d3c8f2aa6436884883b29584e9ac98341155f45ecc102426e` |
| `perf-remeasure-20261103/build-time.txt` | `e348081c645cf0f4b94cbf9b009104ef6a302a9e60f57557103b1fcbe141c42f` |
| `perf-remeasure-20261103/build.log` | `eb40580ec9f028e7f76555486cd05cc423d1bb4a4d3d1fbc66267fbe29c60f49` |
| `perf-remeasure-20261103/measure-time.txt` | `08beaca8c5e2d2c685d5241d9d0ecf0e3dc3a59b7f6e694e92b9d599fcbe6877` |
| `perf-remeasure-20261103/measure.log` | `e7643c4e0d65b0c05937881467d8a2b2c4ddf05a0a6270e4f8a6783afc8367d9` |
| `perf-remeasure-20261103/measure.mjs` | `69cb647ff1742b354ae8546e2efcf002831beec810c33063784fecc84fa18a2d` |
| `perf-remeasure-20261103/samples/1-main.err` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `perf-remeasure-20261103/samples/1-main.json` | `0785da376a03b37cc7ecbe97ba83a6a08405d49b17787c8bee1f9a9f8bd6266a` |
| `perf-remeasure-20261103/samples/1-main.sample.json` | `12703a53b5e4ee806fd19d4e6bd6feccd57ceca2ea02177bad9260848fdc80a0` |
| `perf-remeasure-20261103/samples/2-integration.err` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `perf-remeasure-20261103/samples/2-integration.json` | `c1930c52b66fbe1454c01c5456a5004477b2f13e28a162c45b19a2e07e8e423a` |
| `perf-remeasure-20261103/samples/2-integration.sample.json` | `5bac4401add5556f8a64f42c3cd6e00fd2c8a0d042af9dcf464e56938c2ab19d` |
| `perf-remeasure-20261103/samples/3-integration.err` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `perf-remeasure-20261103/samples/3-integration.json` | `287ab7e379696e2a531fd43129e3be277336422a9f4a57d57e7894aaecec9a91` |
| `perf-remeasure-20261103/samples/3-integration.sample.json` | `88e5016e5a299eebb1e79a51f07ac3be0692f6d36fdd687f3df8c7aaf381f85f` |
| `perf-remeasure-20261103/samples/4-main.err` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `perf-remeasure-20261103/samples/4-main.json` | `87b3ab1266cd8d20208dbfbbc1de8308af00c04f4c2f886bc0cd5e719e343b2b` |
| `perf-remeasure-20261103/samples/4-main.sample.json` | `d17671694ab58f7a2f2e6df016adb092e96550fcd7dca6bcff59fc9024571e7b` |
| `worktree-reinspection-20261104/implement-wasm-restore-tests/implementation.diff` | `b407714b3f6e47502bb521761ff12e16b2e241ddfcb1c4bc12d15bd9b77b5ead` |
| `worktree-reinspection-20261104/implement-wasm-restore-tests/wasm-session-initial-restore.log` | `99583e0bd15820a0af6fd38a146d57e6fd981e9295ad1d29c2e5c7f888c6494d` |
| `worktree-reinspection-20261104/implement-wasm-restore-tests/wasm-worker-ownership.log` | `ccc19a407b551d37f8a606a89478dfd776c59aa84af66d1c9095bc9f52bcc41c` |
| `worktree-reinspection-20261104/implement-wasm-restore-tests/worker-host-w1-bootstrap.log` | `0fe01f25cd7ce4a62546074d02e467ba03b2523932f10c3d43dfca1a2e7cb0ea` |
| `worktree-reinspection-20261104/implement-wasm-restore-tests/实施记录.md` | `1b3dde5c9d3b4554dd2b2cdb8fc3fae6709e0b535eafa24f20f0c6f97e5eb572` |
| `worktree-reinspection-20261104/wait-boundaries/build.jsonl` | `a9e01c7d954a141aecad8666f3e7ab658972dd4e36242a5752546dbac0ae3f8a` |
| `worktree-reinspection-20261104/wait-boundaries/build.stderr` | `d6fa3a603a1080f807d9cb689a082ba8f35057d19034923120a2666114d157c2` |
| `worktree-reinspection-20261104/wait-boundaries/direct-build.log` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `worktree-reinspection-20261104/wait-boundaries/direct-tests.log` | `7654944627f5b3a9b86d9e2a9d225848ce3a2bab85139c0a6724aedfff652425` |
| `worktree-reinspection-20261104/wait-boundaries/实施记录.md` | `fb8ec7297acb7993ad626a457f40c7fb926e95184f21c9d67880b0586b65a48a` |
