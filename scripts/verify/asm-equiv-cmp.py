#!/usr/bin/env python3
"""native 逐指令等价对照：两份 `--emit asm` 产物的函数级多重集比较。

用途
----
验证 §0 硬约束「native 生产构建零损耗」：改动前后的 engine release 汇编
必须在函数级等价。CGU 分区会因代码量变化而漂移（同一函数可能从 cgu.09
挪到 cgu.14），因此**不能按文件或段顺序直接 diff**——本脚本按 mangled
符号聚合函数体，并对 CGU 相关的局部标签与符号哈希归一化后比较。

产出汇编
--------
    CARGO_TARGET_DIR=<dir> cargo +1.96.1 rustc -p engine --release -- --emit asm

两份产物必须用**独立** CARGO_TARGET_DIR，且同工具链同 profile，否则比较无意义。

用法
----
    python3 scripts/verify/asm-equiv-cmp.py <baseline_dir> <candidate_dir>

    # 两个目录下都应有 release/deps/*.s

归一化规则（否则会误报大量差异）
--------------------------------
1. 符号尾部 `17h<16hex>E` → `17hHASHE`：同一函数在不同 CGU 下实例化哈希不同。
2. Rust 局部标签 `.LBB<bb>_<n>` / `.Ltmp<n>` / `.Lanon.<hex>` 会被 CGU 内
   编号重排，比较时抹平。
3. 跳过 `.cfi_*`（调用帧信息）与 `.p2align`（对齐指令）——它们随布局漂移，
   不承载语义，也非性能相关的指令序列。

退出码
------
0 = 函数集合与函数体全部一致
1 = 存在差异（逐条打印）
"""

import glob
import os
import re
import sys

HASH_RE = re.compile(r"17h[0-9a-f]{16}E$")


def load_functions(root: str) -> dict:
    """从 <root>/release/deps/*.s 抽取 {段名: [指令行, ...]}。"""
    funcs: dict = {}
    pattern = os.path.join(root, "release", "deps", "*.s")
    files = glob.glob(pattern)
    if not files:
        sys.exit(f"no .s files under {pattern} — did you pass the CARGO_TARGET_DIR?")
    for path in files:
        text = open(path, errors="replace").read()
        # 每个函数的开始是一个 `.section .text.<mangled>`（可能带引号）
        for chunk in re.split(r'(?m)^\t\.section\t"?\.text\.', text)[1:]:
            name = None
            body = []
            for line in chunk.splitlines():
                if name is None:
                    name = line.split('"', 1)[0].split(",", 1)[0].strip()
                    continue
                s = line.strip()
                if s.startswith(".section"):
                    break
                if not s or s.startswith("#") or s.startswith(".cfi_") or s.startswith(".p2align"):
                    continue
                s = re.sub(r"\.Lanon\.[0-9a-f]+", ".Lanon.X", s)
                s = re.sub(r"\.LBB\d+_\d+", ".LBB", s)
                s = re.sub(r"\.Ltmp\d+", ".Ltmp", s)
                body.append(s)
            if name:
                funcs.setdefault(name, []).extend(body)
    return funcs


def normalize_symbol(name: str) -> str:
    return HASH_RE.sub("17hHASHE", name)


def main() -> int:
    if len(sys.argv) != 3:
        sys.exit(__doc__)

    a_raw = load_functions(sys.argv[1])
    b_raw = load_functions(sys.argv[2])

    def aggregate(raw):
        out: dict = {}
        for name, body in raw.items():
            out.setdefault(normalize_symbol(name), []).extend(body)
        return out

    a, b = aggregate(a_raw), aggregate(b_raw)
    ka, kb = set(a), set(b)
    only_a, only_b = sorted(ka - kb), sorted(kb - ka)
    changed = sorted(k for k in ka & kb if a[k] != b[k])

    print(f"baseline : {len(ka)} functions  ({sys.argv[1]})")
    print(f"candidate: {len(kb)} functions  ({sys.argv[2]})")
    print(f"only_in_baseline={len(only_a)}  only_in_candidate={len(only_b)}  body_changed={len(changed)}")

    if not (only_a or only_b or changed):
        print("\nRESULT: identical")
        return 0

    for label, items in (("only in baseline", only_a), ("only in candidate", only_b), ("body changed", changed)):
        if items:
            print(f"\n-- {label} ({len(items)}) --")
            for name in items[:50]:
                print(f"  {name}")
            if len(items) > 50:
                print(f"  ... and {len(items) - 50} more")

    print("\nRESULT: differs")
    return 1


if __name__ == "__main__":
    sys.exit(main())
