# F3 显式虚拟 preset 最小合同

- 冻结输入：base `3984e14167202dc645ec340fbd2a73e661c71c8a`；ADR-0035 当前禁止证券价格倒推。实读 Rust `company/simple` 仅存在测试 fixture 的金额，无生产默认模板；不将 fixture 升格为批准默认。
- 签名：`createCompanyInitialPreset(template: CompanySystemConfig, seed: bigint, bases: readonly CompanyPeriodBasis[]): Extract<CompanySystemConfig, { mode: "Simple" }>`。
- `CompanyPeriodBasis = { company: number; revenue: AmountRange; fixedExpense: AmountRange; variableExpense: AmountRange | null }`；`AmountRange = { min: string; max: string }`。金额格式严格为 `/^(0|[1-9]\d*)\.\d{2}$/`，拒绝 number、空白、前导零、符号及科学计数；金额全为非负两位小数元字符串、界限不超过 i128::MAX 分，min <= max。
- template 只通过现行 `parseCompanySystemConfig(value,path)`，Simulation 显式拒绝。bases 必须与模板 numeric company 逐一对应、无重复/缺项/额外项；template 本身 company 不重复，company 必须 finite integer u32。该 parser 仅负责结构与字段范围，不证明全部 Rust 领域不变量；helper 不检查开账平衡、期初科目用途、发行人与股票映射或 ratio/noise 联合约束，最终创建仍须通过 Rust 单一领域门禁，不承诺合法 Session。RevenueRatio 要求 variableExpense=null；Growth 要求显式区间。没有隐式生成或补默认。
- 只采样 generation.initial_revenue / initial_fixed_expense / Growth.initial_amount。字段 tag 固定为 `revenue` / `fixedExpense` / `variableExpense`；输入 UTF-8 为 `simple-initial:${company}:${tag}`，company 为规范十进制 numeric u32 文本。以下均 bigint，M=2^64-1：`s=seed; for byte of utf8: s=((s XOR byte)*1099511628211)&M; s=(s+0x9e3779b97f4a7c15)&M; s=((s XOR (s>>30))*0xbf58476d1ce4e5b9)&M; s=((s XOR (s>>27))*0x94d049bb133111eb)&M; sample=s XOR (s>>31)`。名称沿旧 helper 的混合式步骤，不声称采用标准 FNV offset basis。闭区间宽度 `W=maxCents-minCents+1` 必须在 `1..=18446744073709551616`；否则以范围路径显式拒绝。结果 `minCents + sample % W`。虚拟采样不承诺统计无偏。

- 区间直接对应模板所选 settlement_cycle 的完整期间金额，输出采样分对应的两位小数元字符串；不实施任何年化/月度/跨周期规模换算、不引入舍入规则，Rust initial_state 直接使用期间基准。输出仍经结构 parser。
- 其余字段逐值保留，尤其 kind、opening_lines、tax_policy、preferences 与年化趋势/噪声；不采样期初权益，不制造现金、不自动平账。不同周期只重建显式金额，开账金额不按周期缩放。
- seed 必须 bigint 0..u64::MAX；非法模板、区间、公司身份、类型、周期及溢出显式抛路径错误，失败不修改输入。无股票代码、价格或股本输入，因此无价格倒推路径。
- seed-draft 仅持 seed 文本、origin 与 JSON；改 seed 文本不自动重建任何 JSON，显式 regenerate 才调用本 helper。周期切换仅 preset 可显式传入新周期 template 和该周期 ranges，使用同 seed；custom 显式拒绝，要求用户编辑或明确 regenerate。不得从旧周期范围自动推导新金额。创建前 `parseSeedDraft` 严格校验 seed 与 JSON，返回同一 seed，无熵读取。入口 App 接线不属本 writer。

固定 u64 采样向量：`seed=0,company=0,tag=revenue → 443687997363896810`；`seed=42,company=17,tag=fixedExpense → 7775768923652270764`；`seed=18446744073709551615,company=4294967295,tag=variableExpense → 6342113896974490200`。区间退化时采样仍遵循相同步骤。

全部金额以 bigint 分处理；必测 i128::MAX、W=1/2^64/2^64+1、非零 min、超 JS 安全金额、乱序 bases、字段局部性、输入不变与四种 kind/开账原值保留；不同周期使用调用者明确提供的期间范围，不自动换算。
