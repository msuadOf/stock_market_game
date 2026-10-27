# Correction 输入 parser 新 TDD 实施

本树基线 e4db6995。唯一 writer 从真实 Rust DTO/域 codec 与已核 CIP 边界规格重做，没有读取旧 report-corrections.ts；旧版红灯遗失的历史仍不通过，本次不是追认旧实现。

- spec-binding.json 记录真实 Rust 合同、域源码、真实 request fixture、金额 probe 与最初13case冻结测试 SHA。
- 实现前 red-v1 真实 0 pass/13 fail，每个case先固定有效request正向对照，全部因已存在stub显式未实现而失败；不是缺模块、fixture或语法故障。原stub、测试快照、输出和收据原样保存。
- 首次实现 green-v1 真实13/13、lint通过，strict TypeScript因测试两处类型标注失败；修复仅类型标注，见type-fix.md。最初冻结规格不被覆盖，也不声称新文件与旧SHA完全相同。
- green-v2 13/13、strict/noUnused TypeScript（含正式测试）、目标oxlint deny-warnings均通过；三job并发、每命令外部main supervisor10000ms；Node每case10000ms、test-concurrency4。复用既有Node24/工具，不安装依赖。
- parser只接受结构：AccountingAmount为元字符串，Market Money单位不适用；不检查凭证平衡、不分配身份、不审批、不受理、不发布。source仅接受JS可无损表示安全整数，未声称完整u64 JSON数值都可进入Web。
- raw JSON入口必要性为重复key和数值token在JSON.parse之前已不可逆丢失；局部解析器显式拒绝解码后重复key、unsigned负零、小数、指数、unsafe数字以及孤立surrogate；合法pair和特殊own key仍保留。

当前仅writer短验通过，独立完整diff复核尚待结果；未宣称三host、更正生产或完整B/C完成。没有运行Cargo或typegen，没有手写generated。

## 独立复核及 sparse array 修复

未实施本片的 sol_resume_independent_review 发现对象入口的 Array.map 跳过 hole，导致结构通过后 JSON.stringify 产生 Rust 拒绝的 null。先保存单case真实缺少异常红灯（sparse-red，首处失败后短路局限保留），再在实现不变时拆为五个独立case，entries、lines、pending、publications和继承数组元素均实际 Missing expected exception 红（sparse-red-expanded，0/5）。list改为逐index验证Object.hasOwn并解析元素后，green-v3全18/18、strict/noUnused含测试、目标lint均0，各外部10000ms，三job并行。

该未实施reviewer已独立完整复核最终source `3d7bc2b215991dfcf497f81091fc469c321801db01fc081155ccfc2a9a2a0f7e`、test `af2fc363d9ff1f6626db25797c675a6cea8aae42cac4ab2d21e5fe9870244d53`，核对真实Rust DTO/codec/47项enum/日期/元字符串，以及13项最初stub红和sparse五项修前红，确认本次新parser叶片通过；旧版历史TDD缺口没有被追认。此批准仅限输入parser，不扩展为生产更正命令、UI或发布完成。
