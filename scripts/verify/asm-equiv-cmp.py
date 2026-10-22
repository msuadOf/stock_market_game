#!/usr/bin/env python3
"""native 逐指令等价对照：两份 `--emit asm` 产物的函数/数据区比较器。

⚠️ **判据地位（2026-10-22 Sol 审查后降级）**：本工具是 §0「native 零损耗」的
**必要非充分**条件。它只覆盖 `--emit asm` 阶段的 `.s` 内容；最终链接产物的
代码/常量/布局、以及 I-cache/BTB 等真实运行效应，须另以生产 ELF/链接产物检查
+ 条件一致的吞吐测量补足。**本工具报 identical 不构成"已证明逐指令等价"。**

用途
----
比较两份 `cargo rustc -p engine --release -- --emit asm` 的产物，回答：
改动是否引入**任何**机器码/常量/布局层面的差异。

产出汇编（两份须各用**独立** CARGO_TARGET_DIR、同工具链同 profile）：

    CARGO_TARGET_DIR=<dir> cargo +1.96.1 rustc -p engine --release -- --emit asm

用法
----
    python3 scripts/verify/asm-equiv-cmp.py <baseline_dir> <candidate_dir>

退出码
------
0 = 完全无差异（函数体、数据区、规模三项全等）
1 = 存在任何差异（**包括** drop glue / 格式化泛型 / 对齐 / 常量池）
2 = 输入不可信（零解析、无文件、目录错）

分类（business/drop_glue/generic_format）**仅用于报告**，不影响退出码——
按名称豁免会在热路径（每 tick 大量 Vec 分配/释放）上放行真实劣化。

归一化规则（只抹平"纯命名噪声"，不抹平任何语义）
--------------------------------------------------
1. 符号实例哈希 `17h<16hex>E` → `17hHASHE`：同一函数跨 CGU 实例化哈希不同。
2. 局部标签（`.L*`）**按每个文件内首次出现顺序做 α-重命名**：
   合法改名（整体一致重排）判等；**引用身份改变**（如 `jmp .LBB0_1`→`jmp .LBB0_2`，
   两目标语义不同）保留可检出。整段刚性平移不改内部跳转编码，但跨函数距离/
   对齐/链接布局变化可能改变编码——故 `.p2align` **保留比较**，不当噪声删除。
3. 跳过 `#` 注释与 `.cfi_*`（调用帧信息随布局漂移、不承载指令语义）。
4. **不跳过** `.p2align`（对齐影响性能相关布局）。
5. 比较范围除 `.text*` 函数块外，还包括**非 `.text` 数据区块**
   （`.rodata*`/`.gcc_except_table*` 等）的内容多重集——常量池、跳转表、
   LSDA 的字节变化在此层捕获，不再因"函数体没变"而放行。
6. CGU 编号漂移（同一函数从 cgu.09 搬到 cgu.14）通过**全体文件聚合后的
   内容多重集**处理，不做逐 CGU 对齐。

回归（本工具须对以下全部给出非 0 退出，或按要求判等）：
- 纯标签一致改名 ⇒ identical
- `jmp` 目标在两个不同后继之间改变 ⇒ differs
- 被引用的 `.LCPI` 常量值改变 ⇒ differs
- `.p2align 4` → `.p2align 7` ⇒ differs
- drop glue 函数体增加指令 ⇒ differs
- 两份 `.s` 全部解析失败 ⇒ exit 2
"""

import glob
import os
import re
import sys

HASH_RE = re.compile(r"17h[0-9a-f]{16}E")
LABEL_RE = re.compile(r"\.L[A-Za-z0-9_\.]*[A-Za-z0-9_]|\.L")
SECTION_RE = re.compile(r'^\t\.section\t"?([^",\s]+)"?')
CANON_PREFIX = "__LAB"


def canon_labels(lines):
    """按**传入行序列内**的首次出现顺序把 `.L*` 标签重命名为 `__LAB<k>`。

    调用方对每个区块单独调用本函数 ⇒ 重命名作用域是**区块内部**，不是整个文件。
    这一点必须保持：若在文件级重命名，往开头插入一个符号就会把后续所有标签的
    编号整体平移，于是**内容未变的区块也被报成 changed**（假阳性方向）。
    按区块重命名后，每个区块的编号只取决于它自身的内容。
    """
    mapping = {}
    out = []
    for line in lines:
        def repl(m):
            tok = m.group(0)
            if tok not in mapping:
                mapping[tok] = f"{CANON_PREFIX}{len(mapping)}"
            return mapping[tok]
        out.append(LABEL_RE.sub(repl, line))
    return out


def parse_file(path):
    """返回 (funcs, blocks)：符号名/区段名 → 规范化后的内容行列表。

    先按 `.section` 切块，**再对每块单独做标签 α-重命名**（见 `canon_labels`）。
    顺序不能反：先重命名再切块会让编号受整文件影响，产生平移型假阳性。
    """
    sections = []  # [(section_name, [raw line, ...])]
    cur_name, cur_lines = None, []
    with open(path, errors="replace") as fh:
        for line in fh:
            line = line.rstrip("\n")
            if line.lstrip().startswith(".file "):
                continue  # CGU 文件名本身随分区漂移，不承载内容
            m = SECTION_RE.match(line)
            if m:
                if cur_name is not None:
                    sections.append((cur_name, cur_lines))
                cur_name, cur_lines = m.group(1), []
                continue
            if cur_name is not None:
                cur_lines.append(line)
    if cur_name is not None:
        sections.append((cur_name, cur_lines))

    funcs, blocks = {}, {}
    for name, raw_lines in sections:
        canon = HASH_RE.sub("17hHASHE", name)
        bucket = funcs if canon.startswith(".text") else blocks
        body = bucket.setdefault(canon, [])
        for line in canon_labels(raw_lines):
            s = line.strip()
            if not s or s.startswith("#") or s.startswith(".cfi_"):
                continue
            body.append(HASH_RE.sub("17hHASHE", s))
    return funcs, blocks


def load(root):
    pattern = os.path.join(root, "release", "deps", "*.s")
    files = sorted(glob.glob(pattern))
    if not files:
        print(f"INPUT UNTRUSTED: no .s files under {pattern} — wrong CARGO_TARGET_DIR?", file=sys.stderr)
        raise SystemExit(2)
    funcs, blocks = {}, {}
    parsed = 0
    for path in files:
        f, b = parse_file(path)
        if f or b:
            parsed += 1
        for k, v in f.items():
            funcs.setdefault(k, []).append(tuple(v))
        for k, v in b.items():
            blocks.setdefault(k, []).append(tuple(v))
    if parsed == 0:
        print(
            "INPUT UNTRUSTED: 0 files produced any parseable section — parser/format"
            " mismatch; this is NOT evidence of equivalence.",
            file=sys.stderr,
        )
        raise SystemExit(2)
    if parsed != len(files):
        print(f"WARN: parsed {parsed}/{len(files)} .s files — partial coverage")
    return {k: tuple(sorted(v)) for k, v in funcs.items()}, {
        k: tuple(sorted(v)) for k, v in blocks.items()
    }, len(files), parsed


def classify(name):
    if "core..ptr" in name and "drop_in_place" in name or "core3ptr" in name:
        return "drop_glue"
    if "fmt" in name or "box_new_uninit" in name or "core3fmt" in name:
        return "generic_format"
    return "business"


def diff_sets(a, b, label):
    ka, kb = set(a), set(b)
    only_a, only_b = sorted(ka - kb), sorted(kb - ka)
    changed = sorted(k for k in ka & kb if a[k] != b[k])
    total = len(only_a) + len(only_b) + len(changed)
    print(f"{label}: only_base={len(only_a)} only_cand={len(only_b)} changed={len(changed)}")
    for tag, items in ((f"only in baseline", only_a), (f"only in candidate", only_b)):
        for name in items[:20]:
            print(f"  [{tag}] {name[:110]}")
    groups = {}
    for k in changed:
        groups.setdefault(classify(k), []).append(k)
    for kind in ("business", "generic_format", "drop_glue"):
        for name in groups.get(kind, [])[:20]:
            print(f"  [changed:{kind}] {name[:110]}")
    return total


def main():
    if len(sys.argv) != 3:
        print(__doc__)
        return 2
    fa, ba, nf_a, np_a = load(sys.argv[1])
    fb, bb, nf_b, np_b = load(sys.argv[2])
    print(f"baseline : {len(fa)} symbols / {len(ba)} data blocks / {nf_a} files ({sys.argv[1]})")
    print(f"candidate: {len(fb)} symbols / {len(bb)} data blocks / {nf_b} files ({sys.argv[2]})")
    total = diff_sets(fa, fb, "functions(.text)") + diff_sets(ba, bb, "data sections")
    if total == 0:
        print("\nRESULT: identical (necessary condition only — see module docstring)")
        return 0
    print(f"\nRESULT: differs ({total} diff entries; classification is report-only)")
    return 1


if __name__ == "__main__":
    sys.exit(main())
