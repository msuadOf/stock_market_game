# opening_lines AccountingAmount 独立复核修复

- 增量基线：`fef582b83fc0f26b05de98a09d78243bca6fc6c6`。
- 独立 reviewer：`sol61_pending122_contract`，未参与本批实施。发现新 helper 的 `finance.opening_lines[].amount` 复用旧 `parseJournalLine` 后只检查格式，缺失 AccountingAmount 的 i128 范围；`i128::MAX + 1` 分仍被返回而 Rust serde 拒绝。
- 仅修改新 helper 的 opening_lines 投影：保留 `parseJournalLine` 解析结构，随后复用既有 `amount()` 与 `i128()` 检查 canonical 两位小数元字符串及分范围。`journal.ts` 与 `industrial.ts` 未增加行为变化，仍只有前批的 export 改动。
- 依据：`packages/engine/src/accounting/amount.rs:145–207` 与 `:275–279`。Rust 先 checked 构造正绝对值再取负，因此实际可反序列化下界是 `-i128::MAX`，`i128::MIN` 对应负数尾 `.28` 同样拒绝；本层使用无负号分绝对值调用 `i128()`，精确保持这一现行合同，没有修改 Rust。
- 测试新增：正负极值（尾 `.27`）与零原样保留；正 MAX+1、负 MIN、负 MIN−1 显式拒绝并核完整 `setup.company_system.config.companies[0].finance.opening_lines[0].amount` 路径；空白、加号、省略小数、单小数位拒绝。
- `setup-company-opening-red.log`：9 过、2 败；新增溢出与 canonical 格式两例准确因 Missing expected exception 失败，exit 1，wall 632ms。
- `setup-company-opening-green.log`：11/11 全过，exit 0，wall 474ms。每 case `--test-timeout=10000`、`--test-concurrency=4`，整命令与进程树受外部 10000ms deadline supervisor；原始完整 argv/cwd 记在日志第一行。
- `setup-company-opening-validation.log`：完整 tsc 与 oxlint argv/cwd、并发资源与 deadline 在首行。两 child 同时启动，计算共同300000ms deadline剩余量；oxlint另受10000ms且 `--threads=4`，tsc依赖图受工具单进程限制。两项 exit 0，共8375ms。
- 源码 `git diff --check -- apps/web/src/save/schema` 通过。旧红灯日志的原始空白保持，不为通过 diffcheck 改动原件。
- 非公司局真实 producer save 的空投影覆盖限制继续有效；本增量不宣称 Simple 生产样本、完整档或整个组合树通过。未运行 all、merge、push。
- 本修复提交后交同一未实施 reviewer 复核，并交 planweb 精确叠加后绑定最终组合 tree。
