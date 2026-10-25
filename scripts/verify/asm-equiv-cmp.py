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
2 = 输入不可信（零解析、无文件、目录错、标签未定义/重复、
    存在无法归属到 `.section` 的内容——裸 `.text`/`.data`/`.bss` 或首个
    `.section` 之前的实质内容）。此类内容不得静默丢弃或误归前一段。

分类（business/drop_glue/generic_format）**仅用于报告**，不影响退出码——
按名称豁免会在热路径（每 tick 大量 Vec 分配/释放）上放行真实劣化。

归一化规则（只抹平"纯命名噪声"，不抹平任何语义）
--------------------------------------------------
1. 符号实例哈希 `17h<16hex>E` → `17hHASHE`：同一函数跨 CGU 实例化哈希不同。
   **只在引号字符串之外**擦除——字符串字面量里的字节是真实数据，必须逐字节比较。
2. 局部标签（`.L*`）保留**文件内原始定义身份**：定义按所属 section 与块内定义
   序号识别。跨块引用写成 `__REF[<规范 section 名>#<文件内同名序号>:<定义序号>]`，
   不按引用方首次出现编号，因此切换目标不会被抹成同名；同名 section 在同一文件
   内出现多次时按出现序号区分，别名身份保留。不同 CGU 文件中的同名实例互不
   干扰，合法多实例自比较不得退化为 exit 2。
3. 跳过 `#` 注释与 `.cfi_*`（调用帧信息随布局漂移、不承载指令语义）。
4. **不跳过** `.p2align`（对齐影响性能相关布局）。
5. 比较范围除 `.text*` 函数块外，还包括**非 `.text` 数据区块**
   （`.rodata*`/`.gcc_except_table*` 等）的内容多重集——常量池、跳转表、
   LSDA 的字节变化在此层捕获，不再因"函数体没变"而放行。section 属性
   （flags / 类型）作为区块内容首行参与比较。
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
# 匹配完整 symbol，避免把 mangled 名内的 ..LoanState 或字符串字节误当成局部引用。
LABEL_RE = re.compile(r'"(?:\\.|[^"\\])*"|#[^\n]*|(?P<symbol>[A-Za-z_.][A-Za-z0-9_.$]*)')
LABEL_DEF_RE = re.compile(r"^\s*(\.L[A-Za-z0-9_.$]*):")
SECTION_RE = re.compile(r'^\t\.section\t"?([^",\s]+)"?(.*)$')
# 裸 `.text`/`.data`/`.bss` 是 GNU as 的隐式 section 写法；本工具按显式
# `.section` 归属内容，无法安全对齐时必须判输入不可信，而不是静默丢弃或
# 误归到前一个 section。
BARE_SECTION_RE = re.compile(r"^\s*\.(text|data|bss)\s*$")
# `.file` 指令后面可能是制表符或空格；只按指令边界跳过。
FILE_DIRECTIVE_RE = re.compile(r"^\s*\.file\b")
CANON_PREFIX = "__LAB"
QUOTED_RE = re.compile(r'("(?:\\.|[^"\\])*")')


def normalize_hashes(line):
    """只擦符号位置的实例哈希；引号字符串内的字节是数据，必须原样比较。"""
    parts = QUOTED_RE.split(line)
    return "".join(part if index % 2 else HASH_RE.sub("17hHASHE", part)
                   for index, part in enumerate(parts))


def canon_labels(lines, section_index, definitions):
    def repl(m):
        token = m.group("symbol")
        if token is None or not token.startswith(".L"):
            return m.group(0)
        if token not in definitions:
            raise ValueError(f"undefined local label {token}")
        owner, ref_key, ordinal = definitions[token]
        if owner == section_index:
            return f"{CANON_PREFIX}{ordinal}"
        # 外部引用不能按调用方的首次出现编号，否则切换常量目标仍会被抹成同名。
        return f"__REF[{ref_key}:{ordinal}]"

    return [LABEL_RE.sub(repl, line) for line in lines]


def parse_file(path):
    sections = []  # [(section_name, attrs, [raw line, ...])]
    cur_name, cur_attrs, cur_lines = None, "", []
    untrusted = False  # 存在无法归属的内容：不得静默丢弃，也不得误归前一段
    with open(path, errors="replace") as fh:
        for line in fh:
            line = line.rstrip("\n")
            if FILE_DIRECTIVE_RE.match(line):
                continue  # CGU 文件名本身随分区漂移，不承载内容
            m = SECTION_RE.match(line)
            if m:
                if cur_name is not None:
                    sections.append((cur_name, cur_attrs, cur_lines))
                cur_name, cur_attrs, cur_lines = m.group(1), m.group(2), []
                continue
            s = line.strip()
            if BARE_SECTION_RE.match(line):
                # 隐式 section：先闭合前一段，后续内容归属不可安全确定。
                if cur_name is not None:
                    sections.append((cur_name, cur_attrs, cur_lines))
                cur_name, cur_attrs, cur_lines = None, "", []
                untrusted = True
                continue
            if not s or s.startswith("#") or s.startswith(".cfi_"):
                continue
            if cur_name is None:
                # 首个 `.section` 之前的实质内容无处归属。
                untrusted = True
                continue
            cur_lines.append(s)
    if cur_name is not None:
        sections.append((cur_name, cur_attrs, cur_lines))

    # 文件内定义身份：同名 section 按出现序号区分，跨文件同名实例互不干扰。
    canon_sections = []
    occurrences = {}
    for name, attrs, lines in sections:
        canon = HASH_RE.sub("17hHASHE", name)
        occurrence = occurrences.get(canon, 0)
        occurrences[canon] = occurrence + 1
        canon_sections.append((canon, f"{canon}#{occurrence}", attrs, lines))

    definitions = {}
    for index, (_canon, _key, _attrs, lines) in enumerate(canon_sections):
        ordinal = 0
        for line in lines:
            m = LABEL_DEF_RE.match(line)
            if m:
                token = m.group(1)
                if token in definitions:
                    raise ValueError(f"duplicate local label {token}")
                definitions[token] = (index, _key, ordinal)
                ordinal += 1

    funcs, blocks = {}, {}
    for index, (canon, _key, attrs, raw_lines) in enumerate(canon_sections):
        bucket = funcs if canon.startswith(".text") else blocks
        body = bucket.setdefault(canon, [])
        # section 属性（flags/类型）影响链接布局与可见性，作为区块内容首行参与比较。
        body.append("__SECTION_ATTRS__" + normalize_hashes(attrs.strip()))
        for line in canon_labels(raw_lines, index, definitions):
            body.append(normalize_hashes(line))
    return funcs, blocks, untrusted


def load(root):
    pattern = os.path.join(root, "release", "deps", "*.s")
    files = sorted(glob.glob(pattern))
    if not files:
        print(f"INPUT UNTRUSTED: no .s files under {pattern} — wrong CARGO_TARGET_DIR?", file=sys.stderr)
        raise SystemExit(2)
    funcs, blocks = {}, {}
    parsed = 0
    unattributed = []
    for path in files:
        try:
            f, b, untrusted = parse_file(path)
        except ValueError as error:
            print(f"INPUT UNTRUSTED: {path}: {error}", file=sys.stderr)
            raise SystemExit(2) from error
        if untrusted:
            unattributed.append(path)
        if f or b:
            parsed += 1
        for k, v in f.items():
            funcs.setdefault(k, []).append(tuple(v))
        for k, v in b.items():
            blocks.setdefault(k, []).append(tuple(v))
    if parsed == 0:
        # 保留既有“0 files”措辞，并补上无法归属内容的真实原因。
        extra = (
            f"; content outside any .section in: {', '.join(unattributed)}"
            if unattributed else ""
        )
        print(
            "INPUT UNTRUSTED: 0 files produced any parseable section — parser/format"
            f" mismatch{extra}; this is NOT evidence of equivalence.",
            file=sys.stderr,
        )
        raise SystemExit(2)
    if unattributed:
        # 部分内容无法归属时，剩余区块的比较结果不能扩写成“整体等价”。
        print(
            "INPUT UNTRUSTED: content outside any .section in:"
            f" {', '.join(unattributed)} (parsed {parsed}/{len(files)});"
            " cannot attribute all content, this is NOT evidence of equivalence.",
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
