#!/usr/bin/env python3
"""用合成的 .s 验证 asm 比较器；不代表链接 ELF 或吞吐已等价。"""

from concurrent.futures import ThreadPoolExecutor
import os
import re
import shutil
import subprocess
import sys
import time
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
DEFAULT_TOOL = os.path.join(HERE, "asm-equiv-cmp.py")

COUNT_RE = re.compile(
    r"^(?P<label>functions\(\.text\)|data sections):\s*"
    r"only_base=(?P<oa>\d+)\s+only_cand=(?P<ob>\d+)\s+changed=(?P<ch>\d+)",
    re.M,
)
LEGACY_RE = re.compile(
    r"only_in_baseline=(?P<oa>\d+)\s+only_in_candidate=(?P<ob>\d+)\s+"
    r"body_changed=(?P<ch>\d+)",
    re.M,
)

# SECTION_RE 要求 section 行带前导制表符。
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
FN_EXTRA = (
    '\t.section\t".text.Z_extra","ax",@progbits\n'
    "Z_extra:\n"
    ".LBB9_1:\n"
    "\txorl\t%eax, %eax\n"
    "\tjmp\t.LBB9_2\n"
    ".LBB9_2:\n"
    "\tretq\n"
)
FN_LOAD_CONST = (
    '\t.section\t".text.load_const","ax",@progbits\n'
    "load_const:\n"
    "\tmovq\t.Lvalue111(%rip), %rax\n"
    "\tretq\n"
)
CONST_111 = (
    '\t.section\t".rodata.value111","a",@progbits\n'
    ".Lvalue111:\n"
    "\t.quad\t111\n"
)
CONST_222 = (
    '\t.section\t".rodata.value222","a",@progbits\n'
    ".Lvalue222:\n"
    "\t.quad\t222\n"
)
CONST_SECTIONS = CONST_111 + CONST_222
FN_BRANCH = (
    '\t.section\t".text.branch","ax",@progbits\n'
    "branch:\n"
    "\tjmp\t.Lleft\n"
    ".Lleft:\n"
    "\tmovl\t$111, %eax\n"
    "\tretq\n"
    ".Lright:\n"
    "\tmovl\t$222, %eax\n"
    "\tretq\n"
)
# 函数 ↔ 跳转表循环：函数间接跳转，表项再指回函数内的 case 标签。
JUMP_TABLE = (
    '\t.section\t".text.jt_fn","ax",@progbits\n'
    "jt_fn:\n"
    "\tmovl\t%edi, %eax\n"
    "\tjmp\t*.Ljt(%rip)\n"
    ".Lcase0:\n"
    "\tmovl\t$1, %eax\n"
    "\tretq\n"
    ".Lcase1:\n"
    "\tmovl\t$2, %eax\n"
    "\tretq\n"
    '\t.section\t".rodata.jt","a",@progbits\n'
    "\t.p2align\t3\n"
    ".Ljt:\n"
    "\t.quad\t.Lcase0\n"
    "\t.quad\t.Lcase1\n"
)
JUMP_TABLE_RENAMED = (
    JUMP_TABLE
    .replace(".Lcase0", ".Lalpha")
    .replace(".Lcase1", ".Lbeta")
    .replace(".Ljt", ".Ltable")
)
# 两函数共享一个对象 vs 各自指向等值的不同对象。
SHARED_OBJECT = (
    '\t.section\t".text.use_shared_a","ax",@progbits\n'
    "use_shared_a:\n"
    "\tleaq\t.Lshared_value(%rip), %rax\n"
    "\tretq\n"
    '\t.section\t".text.use_shared_b","ax",@progbits\n'
    "use_shared_b:\n"
    "\tleaq\t.Lshared_value(%rip), %rax\n"
    "\tretq\n"
    '\t.section\t".rodata.shared_value","a",@progbits\n'
    ".Lshared_value:\n"
    "\t.quad\t7\n"
)
SEPARATE_EQUAL_OBJECTS = (
    '\t.section\t".text.use_shared_a","ax",@progbits\n'
    "use_shared_a:\n"
    "\tleaq\t.Lshared_a(%rip), %rax\n"
    "\tretq\n"
    '\t.section\t".text.use_shared_b","ax",@progbits\n'
    "use_shared_b:\n"
    "\tleaq\t.Lshared_b(%rip), %rax\n"
    "\tretq\n"
    '\t.section\t".rodata.shared_value_a","a",@progbits\n'
    ".Lshared_a:\n"
    "\t.quad\t7\n"
    '\t.section\t".rodata.shared_value_b","a",@progbits\n'
    ".Lshared_b:\n"
    "\t.quad\t7\n"
)
HASH_LITERAL = (
    FN_A
    + '\t.section\t".rodata.hashlit","a",@progbits\n'
    '\t.asciz\t"17h0123456789abcdefE"\n'
)
# 裸 `.text` 不是 `.section` 行：其下内容无法归属任何 section，必须判输入不可信。
# 刻意放在独立文件里（前面没有 `.section`），复现旧版“整文件内容被静默丢弃、
# 只打 WARN 后仍报 identical”的漏报。
BARE_TEXT = "\t.text\nB_bare:\n\tretq\n"
BARE_TEXT_MUTATED = "\t.text\nB_bare:\n\tmovl\t$1, %eax\n\tretq\n"
UNCHANGED = (0, 0, 0)
CHANGED = (0, 0, 1)


def write_tree(root: str, body: str, filename="engine-cgu.00.rcgu.s") -> str:
    deps = os.path.join(root, "release", "deps")
    os.makedirs(deps, exist_ok=True)
    with open(os.path.join(deps, filename), "w") as fh:
        fh.write('\t.file\t"engine"\n' + body)
    return root


def run_tool(tool: str, base: str, cand: str):
    r = subprocess.run(
        [sys.executable, "-I", tool, base, cand],
        capture_output=True, text=True, timeout=5,
    )
    return r.returncode, r.stdout, r.stderr


def totals(stdout: str):
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


def run_case(tool, tmp, case):
    name, before, after, expected_rc, expected_counts, diagnostic = case
    base = os.path.join(tmp, name, "base")
    cand = base if after is None else os.path.join(tmp, name, "cand")
    for root, bodies in ((base, before), (cand, after)):
        if bodies is None:
            continue
        if isinstance(bodies, str):
            bodies = (bodies,)
        for i, body in enumerate(bodies):
            write_tree(root, body, f"engine-cgu.{i:02d}.rcgu.s")
    started = time.monotonic()
    rc, out, err = run_tool(tool, base, cand)
    elapsed = time.monotonic() - started
    per = totals(out)
    ok = rc == expected_rc
    if expected_counts is not None:
        ok = ok and per == {
            "functions(.text)": expected_counts[0],
            "data sections": expected_counts[1],
        }
    if diagnostic is not None:
        ok = ok and diagnostic in out + err
    if expected_rc != 0:
        ok = ok and "RESULT: identical" not in out
    detail = (
        f"期望 exit={expected_rc} counts={expected_counts}；"
        f"实测 exit={rc} {per} wall={elapsed * 1000:.1f}ms"
    )
    return name, ok, detail, out, err


def main() -> int:
    tool = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_TOOL
    if not os.path.isfile(tool):
        print(f"CASE-HARNESS-ERROR: 比较器不存在：{tool}", file=sys.stderr)
        return 2

    with_refs = FN_A + FN_LOAD_CONST + CONST_SECTIONS
    renamed = with_refs
    for old, new in (
        (".LBB0_1", ".Lalpha"), (".LBB0_2", ".Lbeta"),
        (".Lvalue111", ".Lconstant_x"), (".Lvalue222", ".Lconstant_y"),
    ):
        renamed = renamed.replace(old, new)
    same_section = CONST_SECTIONS.replace(
        '\t.section\t".rodata.value222","a",@progbits\n', ""
    )
    ambiguous = CONST_SECTIONS.replace(
        ".rodata.value111", ".rodata.values"
    ).replace(".rodata.value222", ".rodata.values")
    aligned = FN_A.replace("A_fn:\n", "\t.p2align\t4\nA_fn:\n")
    drop_glue = FN_A.replace("A_fn", "_ZN4core3ptr13drop_in_place17h1111111111111111E")
    mangled_name = (
        "_ZN4core3ptr115drop_in_place$LT$$LP$engine..company..contracts..ContractId$C$"
        "engine..company..industrial..loans..LoanState$RP$$GT$17hc9f92f5f3586909dE"
    )
    mangled_fn = FN_A.replace("A_fn", mangled_name)
    ordinary_ref = FN_A.replace("\tretq\n", f"\tcallq\t{mangled_name}\n\tretq\n")
    literal = FN_A + '\t.section\t".rodata.literal","a",@progbits\n\t.asciz\t".Lliteral_text"\n'
    # counts 固定同时核对函数与数据，避免退出码正确却把差异算到错误区块。
    cases = [
        ("insert_symbol_at_front", FN_A + FN_B, FN_EXTRA + FN_A + FN_B,
         1, ((0, 1, 0), UNCHANGED), None),
        ("identical_input", FN_A + FN_B, None,
         0, (UNCHANGED, UNCHANGED), None),
        ("real_body_change", FN_A + FN_B, FN_A_MUTATED + FN_B,
         1, (CHANGED, UNCHANGED), None),
        ("cross_section_reference_target_change", FN_LOAD_CONST + CONST_SECTIONS,
         FN_LOAD_CONST.replace(".Lvalue111", ".Lvalue222") + CONST_SECTIONS,
         1, (CHANGED, UNCHANGED), None),
        ("consistent_label_rename", with_refs, renamed,
         0, (UNCHANGED, UNCHANGED), None),
        ("insert_symbol_with_cross_references", with_refs, FN_EXTRA + with_refs,
         1, ((0, 1, 0), UNCHANGED), None),
        ("jump_target_change", FN_BRANCH, FN_BRANCH.replace("jmp\t.Lleft", "jmp\t.Lright"),
         1, (CHANGED, UNCHANGED), None),
        ("referenced_constant_bytes_change", FN_LOAD_CONST + CONST_SECTIONS,
         FN_LOAD_CONST + CONST_SECTIONS.replace(".quad\t111", ".quad\t112"),
         1, (UNCHANGED, CHANGED), None),
        ("data_bytes_change", FN_A + CONST_SECTIONS,
         FN_A + CONST_SECTIONS.replace(".quad\t222", ".quad\t333"),
         1, (UNCHANGED, CHANGED), None),
        ("function_alignment_change", aligned, aligned.replace(".p2align\t4", ".p2align\t7"),
         1, (CHANGED, UNCHANGED), None),
        ("data_alignment_change", FN_LOAD_CONST + CONST_SECTIONS,
         FN_LOAD_CONST + CONST_SECTIONS.replace(".Lvalue111:\n", "\t.p2align\t7\n.Lvalue111:\n"),
         1, (UNCHANGED, CHANGED), None),
        ("drop_glue_extra_instruction", drop_glue, drop_glue.replace("pushq\t%rbp", "pushq\t%rbp\n\tnop"),
         1, (CHANGED, UNCHANGED), "[changed:drop_glue]"),
        ("zero_parse", "\t.text\n\tretq\n", None,
         2, None, "INPUT UNTRUSTED: 0 files"),
        ("second_definition_in_target_section", FN_LOAD_CONST + same_section,
         FN_LOAD_CONST.replace(".Lvalue111", ".Lvalue222") + same_section,
         1, (CHANGED, UNCHANGED), None),
        # 同名 section 是合法多实例：文件内定义身份足以区分，不得退化为 exit 2。
        ("same_name_targets_within_file_self_compare", FN_LOAD_CONST + ambiguous, None,
         0, (UNCHANGED, UNCHANGED), None),
        ("same_name_target_across_files_self_compare",
         (FN_LOAD_CONST + CONST_111, CONST_222.replace(".rodata.value222", ".rodata.value111")), None,
         0, (UNCHANGED, UNCHANGED), None),
        # (a) 跨 section 自比较为 0；只换引用目标、数据不变为 1（见 cross_section_reference_target_change）。
        ("self_compare_with_cross_references", with_refs, None,
         0, (UNCHANGED, UNCHANGED), None),
        # (b) 函数↔跳转表循环。
        ("jump_table_cycle_consistent_rename", JUMP_TABLE, JUMP_TABLE_RENAMED,
         0, (UNCHANGED, UNCHANGED), None),
        ("jump_table_successor_change", JUMP_TABLE,
         JUMP_TABLE.replace("\t.quad\t.Lcase1\n", "\t.quad\t.Lcase0\n"),
         1, (UNCHANGED, CHANGED), None),
        ("jump_table_function_preinsert", JUMP_TABLE, FN_EXTRA + JUMP_TABLE,
         1, ((0, 1, 0), UNCHANGED), None),
        # (c) 共享别名 vs 等值独立对象：必须检出，绝不判等。
        ("shared_alias_replaced_by_equal_distinct_objects",
         SHARED_OBJECT, SEPARATE_EQUAL_OBJECTS,
         1, ((0, 0, 2), (1, 2, 0)), None),
        ("shared_alias_self_compare", SHARED_OBJECT, None,
         0, (UNCHANGED, UNCHANGED), None),
        ("equal_distinct_objects_self_compare", SEPARATE_EQUAL_OBJECTS, None,
         0, (UNCHANGED, UNCHANGED), None),
        # (d) 引号字符串里的实例哈希字节是数据，不得被归一化擦除。
        ("quoted_hash_bytes_change", HASH_LITERAL,
         HASH_LITERAL.replace("17h0123456789abcdefE", "17hfedcba9876543210E"),
         1, (UNCHANGED, CHANGED), None),
        ("quoted_hash_bytes_self_compare", HASH_LITERAL, None,
         0, (UNCHANGED, UNCHANGED), None),
        # section 属性（flags）影响链接布局，必须参与比较。
        ("section_attribute_change", FN_A, FN_A.replace('"ax",@progbits', '"a",@progbits'),
         1, (CHANGED, UNCHANGED), None),
        ("undefined_local_reference", FN_LOAD_CONST, None,
         2, None, "undefined local label"),
        ("duplicate_local_definition", FN_LOAD_CONST + CONST_SECTIONS + CONST_111, None,
         2, None, "duplicate local label"),
        ("mangled_symbol_with_local_label_substring", mangled_fn, None,
         0, (UNCHANGED, UNCHANGED), None),
        ("ordinary_symbol_reference_change", ordinary_ref,
         ordinary_ref.replace("..LoanState", "..LoanStatus"),
         1, (CHANGED, UNCHANGED), None),
        ("quoted_local_label_text", literal, None,
         0, (UNCHANGED, UNCHANGED), None),
        ("quoted_local_label_text_change", literal, literal.replace(".Lliteral_text", ".Lother_text"),
         1, (UNCHANGED, CHANGED), None),
        # 无法归属的内容不得静默丢弃：exit 0 必须意味着全部输入区块都已比较。
        ("content_outside_section_is_untrusted",
         (FN_A, BARE_TEXT), (FN_A, BARE_TEXT_MUTATED),
         2, None, "content outside any .section"),
    ]
    tmp_root = os.environ.get("TMPDIR", os.path.join(REPO, ".tmp"))
    os.makedirs(tmp_root, exist_ok=True)
    tmp = tempfile.mkdtemp(prefix="asm-equiv-regression-", dir=tmp_root)
    workers = min(len(cases), len(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else (os.cpu_count() or 1))
    print(f"被测比较器：{tool}\n并发 workers={workers} cases={len(cases)} child_timeout=5s", flush=True)
    try:
        with ThreadPoolExecutor(max_workers=workers) as executor:
            results = list(executor.map(lambda case: run_case(tool, tmp, case), cases))
    finally:
        shutil.rmtree(tmp)

    print("=" * 68)
    failed = 0
    for name, ok, detail, out, err in results:
        print(f"[{'PASS' if ok else 'FAIL'}] {name}: {detail}")
        if not ok:
            failed += 1
            print("  -- 比较器原始输出 --")
            print(out, end="")
            print(err, end="", file=sys.stderr)
    print("=" * 68)
    if failed:
        print(f"RESULT: {failed}/{len(results)} 用例未通过 —— 比较器存在漏报、误报或行为不符")
        return 1
    print(f"RESULT: {len(results)}/{len(results)} 用例通过")
    return 0


if __name__ == "__main__":
    sys.exit(main())
