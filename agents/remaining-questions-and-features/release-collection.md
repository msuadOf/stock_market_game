# Q18：最终 Release collector 的最低格式检查

用户逐项讨论后选择“最终收集器也检查”。此前 producer 已拒绝缺失格式，collector
则仅检查十组产品/平台身份、文件清单、大小和 SHA-256；没有正常发布绕过 producer
的证据，本项是新增最终收集防御，不将历史 Release 描述为已经漏包。

## 实现范围

`package-distributions.mjs` 提取 `desktopInstallerFormats`，由输入检查与纯守卫
`requireDistributionFormats` 共用；原生 producer 在发布清单前调用守卫，
`publish-release.mjs` 的 collector 在创建输出目录前复用同一守卫。

- Windows Desktop：MSI、NSIS、便携 ZIP；Linux Desktop：DEB、RPM、AppImage、便携 ZIP。
- macOS Desktop：DMG、app ZIP、app tar.gz；三平台 Desktop 清单包含 LICENSE。
- Server、WebUI Server、静态 Web：对应产品/target 的 ZIP 和 tar.gz。
- 保留额外合法安装器、不新增每平台产品配额；手动单产品仍只检查自身产品/平台。
- 不改变 unsigned、标签、源码 SHA、存档、交易或资金/股份语义；不做版本兼容。

## TDD 与短验证

先将 collector fixture 补齐现行产品格式，新增“实际文件与 manifest 一致但删除任一
最低格式”用例。旧实现的定向红测得到 `Missing expected rejection`：同步删除 Linux
DEB 及清单条目后，原 collector 仍成功。实现后验证缺格式必拒，且输出目录尚不存在。
保留原有摘要篡改、符号链接、重名、tag 移动与 draft 上传失败断言；另用纯 helper
矩阵验证每个平台以及 Server/WebUI Server 的独立产品组合。

使用 Node case timeout 10000ms 和外部进程树 deadline 10000ms，测试并发参数为 4。
代表性命令（分命令执行，避免扩大普通测试期限）：

```sh
node scripts/run-with-deadline.mjs 10000 -- node --test --test-isolation=none --test-timeout=10000 --test-concurrency=4 scripts/package-distributions.test.mjs
node scripts/run-with-deadline.mjs 10000 -- node --test --test-isolation=none --test-timeout=10000 --test-concurrency=4 scripts/publish-release.test.mjs
```

测试使用临时小 fixture 与 mock GitHub 调用；没有实际创建或上传 Release、没有 push、
没有执行完整回归或三平台实际发包。上述两条命令分别通过 22 与 5 个 case，实测
wall-clock 为 0.37s 与 0.93s。

## 独立复核

非作者 `release_collection_review` 完整读取指定脚本/测试 diff、正式部署文档与本工作记录，
并核对 ADR-0028 及静态 Web producer。结论无阻断发现：大 A 语义不变；复用纯守卫，
范围必要且最小；缺格式拒绝发生在创建输出目录前，原有错误边界断言保留；手动单产品
及额外合法安装器仍允许，不增加签名、兼容、平台配额或其他构建。其
`git diff --check` 通过，未重新执行测试，不把作者短测记录当作独立实测。

root另安排非作者 `review_q18_collector` 全文核对最终producer/collector、两套测试及正式文档，
三项门禁通过；其独立并发短测22/22与5/5通过，各命令10000ms外部deadline、case10000ms。
仍未运行真实Release、全部脚本集或完整回归；复制阶段I/O故障的既存本地部分目录边界不
冒称由本次格式守卫全面解决。
