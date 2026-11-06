# holiday-gate 历史证据归档

来源为 `holiday-gate` 工作树根目录中的五份文件；采集时该工作树 HEAD 为
`4afd4920762b952804abfd5b6f89323aad71715e`。这些文件是历史运行输出，不能证明当前版本的验证结果。
`.tmp-baseline-build.log` 原本被 Git 忽略，其余文件的来源同样是工作树文件，均未改写源文件。

归档只把本机工作树绝对路径替换为 `<WORKTREE>`，把本机临时目录前缀替换为
`<TMP>/forensic-final`；其余字节保留。以下 SHA-256 分别对应采集时的源文件字节与归档字节。

| 文件 | 源 SHA-256 | 归档 SHA-256 |
|------|------------|--------------|
| `.tmp-fail-all.txt` | `5cc9e861f2b3a2da3a52b50a56ad58ca3cac4c24e5f1eb57e5f2278b0f2dafcb` | `5cc9e861f2b3a2da3a52b50a56ad58ca3cac4c24e5f1eb57e5f2278b0f2dafcb` |
| `.tmp-fail-lib.txt` | `2b9d1ccb6dc66d867113b24e190e54f08f18cd4591b5b1c7f177f22464d9e54a` | `2b9d1ccb6dc66d867113b24e190e54f08f18cd4591b5b1c7f177f22464d9e54a` |
| `.tmp-baseline-build.log` | `667675abc8cfb69fe46277fad5aef35554b46874cd59cec95988fdc51ece7a25` | `4bb43f03ec45e6090dddc89111ba67e4c9e915ef846f6e6166264ba8f40dbb0b` |
| `.tmp-full-engine.log` | `bc7327c9dac858d4eb4b61a7245ff92a304ef555f63ae19da88fdb0fa22d5796` | `2a1a23fe5fc32b13357295c8974c8f22078cdde48cf09d50e1a6add2b1029e3a` |
| `.tmp-full-nff.log` | `5cb28df84ad6342227e0cc0cc64e9f2c1960ff216c596f34d84067b729dd20f5` | `44da196805165775429c952b094608a7eddfd15175db31fe1fbcfacaffc7aaad` |

两份失败摘要分别记录 62 项和 82 项失败；完整 engine 和 nff 日志均以 `EXIT=101` 结束。
构建日志以 `EXIT=0` 结束，仅记录当时构建成功，不能推导测试通过。归档未重新执行这些命令。
