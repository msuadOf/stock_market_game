# Release 公开前 tag SHA 复核：TDD 记录

日期：2026-10-03。修改范围仅 `scripts/publish-release.mjs` 和 `scripts/publish-release.test.mjs`；Git branch 为 `refactor/oop-complete`。未修改他人已有改动，未提交或真实发布。

## 需求与语义

draft 资产上传及核验可能耗时；原实现只在发布函数开始时查 tag SHA。现在在远端资产名称/bytes/SHA-256 验证结束后、`gh release edit --draft=false` 前再次查询 tag 对应 commit。如果与冻结的 RELEASE_SHA 不符，显式抛出中文错误，保留 draft，不发公开命令。查询本身失败也直接传播错误，不静默 fallback。

此变更只保护发布源码绑定，不改变 A 股 engine、板块规则、资金/股数单位或存档/API 语义。新增查询与 GitHub 公开操作仍是两个请求，没有声称提供原子 tag 锁；它覆盖上传期间已发生且复核时可见的 tag 变化。

## 红灯

先新增测试“上传 draft 期间 tag 移动时必须保持 draft 并拒绝公开 Release”。mock 第一次 commit 查询返回 RELEASE_SHA，第二次返回另一个 SHA；成功 draft 与资产 digest 校验仍保留。

在未修改实现时运行：

```sh
node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=4 --test-reporter=tap scripts/publish-release.test.mjs
```

exit 1，原有 3 case 通过，新增 case 因 `Missing expected rejection.` 失败。旧实现打印 `Published test-abc ...`，证明错误路径是上传后仍公开 draft，而不是编译或 import 错误。日志：[publish-tag-sha-red.log](publish-tag-sha-red.log)。

## 绿灯

实现增加 4 行公开前 SHA 复核。成功路径原有 edit/draft=false 断言保留，并新增准确调用断言，确认第二次 tag 查询发生在 edit 前。

```sh
node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=4 --test-reporter=tap scripts/publish-release.test.mjs scripts/release-policy.test.mjs scripts/distribution-workflow.test.mjs
```

exit 0，15/15 case 通过，0 failed/skipped/cancelled，Node 报告执行 2643.35ms。最大 Node file workers 4、每 case timeout 10000ms、外部整命令进程树 deadline 10000ms；通过工具 approval review 在沙箱外执行，未调用真实 GitHub。

`git diff --check -- scripts/publish-release.mjs scripts/publish-release.test.mjs` 通过。日志：[publish-tag-sha-green.log](publish-tag-sha-green.log)。

独立 subagent 完整 diff 审查由 root 安排；此记录不替代该门禁。
