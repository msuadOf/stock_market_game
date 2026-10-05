# 当前真实日终 fixture 候选独立复核

## 结论与范围

复核者没有生成或安装候选、修改 JSON／实现、执行 Cargo 或 WASM／Browser 验收。
已完整读取 `current-save-fixture-generator.rs`、实际候选的结构与事实、生成及严格解析
日志，独立执行当前 `parseSaveJson` 与 `validateDayEndArchive`。候选严格有效性与
原 ProtocolSession producer 路径的限定门禁通过，可安装下列整个产物及公司切片。
这不代表日终测试套件全绿，也不代表 Bank、完整行业、跨宿主或完整回归完成。

## 真实 producer 与原场景

Root 统一 release 构建日志为 `.tmp/ui-design-merge/current-fixture-release-build.stderr`，
实际记录成功、40.67 秒。生成者确认使用当前 `target/release/libengine.rlib` 与
`libserde_json-7d4549fd2c9db610.rlib` 链接 release producer，并以
`RAYON_NUM_THREADS=8 node scripts/run-with-deadline.mjs 10000 --` 运行，exit 0。
`.tmp/ui-design-merge/current-schema-save-candidate-run.log` 无 stdout；空日志本身不
证明 exit 0，退出状态依据生成者实际工具 session 98921 的结果，不能称空日志为绿色。

完整 producer 源码只借旧 fixture 的 setup/seed，新建 `ProtocolSession`；公司状态、
groups 与现行配置由明确新局输入重新生成，没有读旧档后补字段的兼容路径。固定
seed 666959854、5 证券、12 Retail／10 Inst／4 Hot、60 tick／日；两个真实日终后
tick 120。先保存、真实 `ProtocolSession::restore`、再次 save JSON 深等值；再把
不中断与恢复会话各推进 3 tick，检查 tick／seq 连续、各自真实 NPC OrderAccepted
及 receipt cursor 不回退，最后写原日终 save。检查发生在写文件之前，失败不会
发布此候选。不以自由调度下两分支逐笔交易完全相同作断言。

## 产物与独立检查

- 候选：`.tmp/ui-design-merge/current-schema-save-candidate.json`。
- 实际 SHA256：`1f58e3c7f602f8b5adaa8b16b69794eac3df5434e6a7b2d646c17af4289ff733`。
- 公司切片：`.tmp/ui-design-merge/current-company-slice-candidate.json`。
- 实际 SHA256：`4e38ef7d76c5ab9cc9ef50554ce959c9b9a6c0c7eaaee582932e7a004dac43a8`。

独立 `sha256sum` 均匹配；另用 Node 深等值检查切片精确等于完整候选的
`company_operations`，没有裁剪 Journal、手补字段或伪造行业。
独立使用 10000ms 外部命令树 deadline 运行当前严格 parser＋日终 validator，exit 0，
实际约 0.52 秒，输出 tick 120、settledThrough 2030-01-08。

原始 JSON 中 `pending_npc=null`，active daily candles 与 runtime active minute history
为空；`retained_market_history` 完整覆盖 startDate 2030-01-07 至 settledThrough
2030-01-08，每日均覆盖全部 5 证券。严格 parser 同时核验真实分钟累计与 daily candle
成交统计，不靠清空 retained 或默认补齐过关。local-owner 的 account_id 为规范字符串
`"0"`，admission external cash 与该自定义 setup 的 starting cash 一致；它不是默认
100 亿元开局场景。现行 owner／claim 等契约由严格当前 parser 接受，不手工造字段。
五家公司均为 Industrial，Bank books 为 0，因此本候选没有证明 Bank 存档覆盖。

## 必须保留的真实失败

已亲读 `.tmp/ui-design-merge/current-schema-save-eod-tests.log`：7 case 中 1 通过、
6 失败。六项失败发生于测试 `archive()` 的旧 synthetic 输入未提供完整 retained
历史，进入目标坏档断言之前已被严格 parser 拒绝；不能报告整套通过，也不能通过
放松 parser 或篡改真实候选来绕过。此候选安装不会自动修复 synthetic fixture，
须由对应作者修正合法基线并独立复核，原目标坏档断言应保留。

## 语义与安装边界

更新测试 fixture 保留真实资金、股份、T+1、费用、日终委托清理与已发生分钟成交事实，
不改 A 股交易制度、不补钱、不引入版本兼容。整个真实产物替换过时 fixture 是必要
范围；公司 slice 只替换相同公司状态的投影。安装者须再次核对仓库目标文件哈希及
切片深等值；不得把本次限定候选有效性复核扩张为全测试、WASM 或 Browser 绿色。
