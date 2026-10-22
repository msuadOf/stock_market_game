#!/usr/bin/env python3
"""asm 对照比较器的回归用例（构造输入，无需真实 cargo 产物）。

背景
----
`asm-equiv-cmp.py` 的标签归一化若以**整个文件**为作用域按首次出现顺序编号，
则"在文件开头插入一个函数"会把该文件后续**所有**标签编号整体平移 ⇒ 所有已有
函数体被误报为 changed。真实产物上这会把 4 条真实差异放大成 15311 条，
严重误导后人（详见 `agents/wasm-thread-r4a/phase2-ledger.md` §6.2）。

本用例用**合成的** `.s` 钉住该行为：两份输入只差一个无关函数，其余函数体
逐字节相同 ⇒ 正确的比较器**只能**报 1 条差异（多出的那个符号）。

用法
----
    # 默认测 scripts/verify/asm-equiv-cmp.py
    python3 scripts/verify/asm-equiv-cmp-regression.py

    # 指定被测比较器（例如当前分支上的新版副本 cmp2.py）
    python3 scripts/verify/asm-equiv-cmp-regression.py scripts/verify/asm-equiv-cmp2.py

退出码
------
0 = 全部用例通过（无假阳性）
1 = 至少一个用例未通过（比较器存在假阳性/行为不符）
2 = 用例自身无法运行（比较器缺失、无法执行）

用例清单
--------
1. insert_symbol_at_front
   期望：only_cand=1（仅 Z_extra），changed=0（A_fn/B_fn 不得被报改动）
2. identical_input
   期望：0 差异（同一目录自比较）
3. real_body_change
   期望：changed>=1（真改动必须能检出，防止为消假阳性而把真差异也抹掉）
"""

import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
DEFAULT_TOOL = os.path.join(HERE, "asm-equiv-cmp.py")

# 比较器输出里抽取计数的正则。
# 新版（e405ca40 起）逐段打印：
#   "functions(.text): only_base=0 only_cand=1 changed=2"
COUNT_RE = re.compile(
    r"^(?P<label>functions\(\.text\)|data sections):\s*"
    r"only_base=(?P<oa>\d+)\s+only_cand=(?P<ob>\d+)\s+changed=(?P<ch>\d+)",
    re.M,
)
# 旧版只打一行汇总：
#   "only_in_baseline=0  only_in_candidate=1  body_changed=0 (business=..., ...)"
LEGACY_RE = re.compile(
    r"only_in_baseline=(?P<oa>\d+)\s+only_in_candidate=(?P<ob>\d+)\s+"
    r"body_changed=(?P<ch>\d+)",
    re.M,
)

# 注意：比较器的 SECTION_RE 要求 `^\t\.section\t`，故 section 行必须带前导制表符
FN_A = (
    '\t.section\t".text.A_fn","ax",@progbits\n'
    "A_fn:\n"
    ".LBB0_1:\n"
    "\tpushq\t%rbp\n"
    "\tjmp\t.LBB0_2\n"
    ".LBB0_2:\n"
    "\tpopq\t%rbp\n"
    "\tretq\n"
)

# 与 FN_A 同构但内容不同：用于"真改动必须能检出"用例
FN_A_MUTATED = FN_A.replace("pushq\t%rbp", "pushq\t%rbp\n\tnop")

FN_B = (
    '\t.section\t".text.B_fn","ax",@progbits\n'
    "B_fn:\n"
    ".LBB1_1:\n"
    "\tmovl\t$1, %eax\n"
    "\tjmp\t.LBB1_2\n"
    ".LBB1_2:\n"
    "\tretq\n"
)

# candidate 多出来的无关函数，放在**文件最前面** ⇒ 其后所有标签编号整体平移
FN_EXTRA = (
    '\t.section\t".text.Z_extra","ax",@progbits\n'
    "Z_extra:\n"
    ".LBB9_1:\n"
    "\txorl\t%eax, %eax\n"
    "\tjmp\t.LBB9_2\n"
    ".LBB9_2:\n"
    "\tretq\n"
)


def write_tree(root: str, body: str) -> str:
    """写一个最小 CARGO_TARGET_DIR 形态的目录：root/release/deps/*.s"""
    deps = os.path.join(root, "release", "deps")
    os.makedirs(deps, exist_ok=True)
    with open(os.path.join(deps, "engine-cgu.00.rcgu.s"), "w") as fh:
        fh.write('\t.file\t"engine"\n' + body)
    return root


def run_tool(tool: str, base: str, cand: str):
    """运行比较器，返回 (returncode, stdout, stderr)。"""
    r = subprocess.run(
        [sys.executable, tool, base, cand], capture_output=True, text=True
    )
    return r.returncode, r.stdout, r.stderr


def totals(stdout: str):
    """汇总各段计数：{label: (only_base, only_cand, changed)}。

    兼容新旧两种输出格式：新版逐段打印，旧版只有一行汇总（且旧版**不比较**
    数据区块，故其"合计"天然只覆盖 .text——这是旧版自身的覆盖缺口，由
    `asm-equiv-cmp.py` 修，不属本回归用例的判据）。
    """
    per = {}
    for m in COUNT_RE.finditer(stdout):
        per[m.group("label")] = (
            int(m.group("oa")),
            int(m.group("ob")),
            int(m.group("ch")),
        )
    if not per:
        m = LEGACY_RE.search(stdout)
        if m:
            per["legacy(.text only)"] = (
                int(m.group("oa")),
                int(m.group("ob")),
                int(m.group("ch")),
            )
    return per


def main() -> int:
    tool = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_TOOL
    if not os.path.isfile(tool):
        print(f"CASE-HARNESS-ERROR: 比较器不存在：{tool}", file=sys.stderr)
        return 2

    tmp = tempfile.mkdtemp(prefix="asm-equiv-regression-")
    results = []
    try:
        # ---- 用例 1：开头插入一个无关函数，已有函数体不得被报改动 ----
        a = write_tree(os.path.join(tmp, "c1_base"), FN_A + FN_B)
        b = write_tree(os.path.join(tmp, "c1_cand"), FN_EXTRA + FN_A + FN_B)
        rc, out, err = run_tool(tool, a, b)
        per = totals(out)
        if not per:
            print(f"CASE-HARNESS-ERROR: 无法解析比较器输出\n{out}\n{err}", file=sys.stderr)
            return 2
        oa = sum(v[0] for v in per.values())
        ob = sum(v[1] for v in per.values())
        ch = sum(v[2] for v in per.values())
        ok = (ob == 1 and oa == 0 and ch == 0)
        results.append(
            (
                "insert_symbol_at_front",
                ok,
                f"期望 only_base=0 only_cand=1 changed=0；实测 "
                f"only_base={oa} only_cand={ob} changed={ch}",
            )
        )
        if not ok:
            print("  -- 比较器输出 --")
            print("\n".join("     " + ln for ln in out.strip().splitlines()[:20]))

        # ---- 用例 2：同输入自比较，必须零差异 ----
        a2 = write_tree(os.path.join(tmp, "c2"), FN_A + FN_B)
        rc2, out2, err2 = run_tool(tool, a2, a2)
        per2 = totals(out2)
        tot2 = sum(sum(v) for v in per2.values()) if per2 else -1
        results.append(
            (
                "identical_input",
                tot2 == 0,
                f"期望 0 差异；实测合计 {tot2}",
            )
        )

        # ---- 用例 3：真实函数体改动必须能检出（防过度归一化）----
        a3 = write_tree(os.path.join(tmp, "c3_base"), FN_A + FN_B)
        b3 = write_tree(os.path.join(tmp, "c3_cand"), FN_A_MUTATED + FN_B)
        rc3, out3, err3 = run_tool(tool, a3, b3)
        per3 = totals(out3)
        ch3 = sum(v[2] for v in per3.values()) if per3 else -1
        results.append(
            (
                "real_body_change",
                ch3 >= 1,
                f"期望 changed>=1（真改动必须检出）；实测 changed={ch3}",
            )
        )
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    print(f"被测比较器：{tool}")
    print("=" * 68)
    failed = 0
    for name, ok, detail in results:
        print(f"[{'PASS' if ok else 'FAIL'}] {name}: {detail}")
        if not ok:
            failed += 1
    print("=" * 68)
    if failed:
        print(f"RESULT: {failed}/{len(results)} 用例未通过 —— 比较器存在假阳性或行为不符")
        return 1
    print(f"RESULT: {len(results)}/{len(results)} 用例通过")
    return 0


if __name__ == "__main__":
    sys.exit(main())
