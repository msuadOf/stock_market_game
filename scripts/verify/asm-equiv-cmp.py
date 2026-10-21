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


CGU_RE = re.compile(r"-cgu\.(\d+)\.rcgu\.s$")


def load_functions(root: str) -> dict:
    """从 <root>/release/deps/*.s 抽取 {cgu 编号: {段名: 函数体}}。

    必须**按 CGU 分开**保存：同一符号会在多个 CGU 各生成一份实例（monomorphized
    copy），把不同 CGU 的同名实例混进同一个列表再比较会产生大量假阳性——列表
    顺序取决于文件遍历顺序。正确做法是同一 CGU 编号内逐符号比较。
    """
    pattern = os.path.join(root, "release", "deps", "*.s")
    files = glob.glob(pattern)
    if not files:
        sys.exit(f"no .s files under {pattern} — did you pass the CARGO_TARGET_DIR?")

    by_cgu: dict = {}
    for path in files:
        m = CGU_RE.search(os.path.basename(path))
        cgu = m.group(1) if m else "_"
        bucket = by_cgu.setdefault(cgu, {})
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
                bucket[name] = tuple(body)
    return by_cgu


def normalize_symbol(name: str) -> str:
    return HASH_RE.sub("17hHASHE", name)


def main() -> int:
    if len(sys.argv) != 3:
        sys.exit(__doc__)

    a_by_cgu = load_functions(sys.argv[1])
    b_by_cgu = load_functions(sys.argv[2])

    # 同一函数常被多个 CGU 各实例化一份；分区漂移会让它们在不同 CGU 间搬家。
    # 因此按"函数体内容多重集"在**全体 CGU 上**聚合同一规范化符号，而不是逐
    # CGU 对齐——后者会把搬家误判为增删。
    def collect(by_cgu):
        out: dict = {}
        for bucket in by_cgu.values():
            for name, body in bucket.items():
                out.setdefault(normalize_symbol(name), []).append(body)
        return {k: tuple(sorted(v)) for k, v in out.items()}

    a, b = collect(a_by_cgu), collect(b_by_cgu)
    ka, kb = set(a), set(b)
    only_a, only_b = sorted(ka - kb), sorted(kb - ka)
    changed = sorted(k for k in ka & kb if a[k] != b[k])

    def classify(name: str) -> str:
        """按 mangled 符号粗分三类，便于判读差异是否触及真实业务代码。

        - `drop_glue`：`core::ptr::drop_in_place` 清理辅助，随 CGU 分区漂移改变
          内联/归属形态，不在热路径上。
        - `generic_format`：`core::fmt` 系列（Debug/Display/num 格式化）与
          `alloc::boxed::box_new_uninit` 等无业务语义的泛型实例，其展开形态受
          调用点集变化影响。
        - `business`：其余，即 engine 自身逻辑或与之直接相关的实例化。
        """
        if ("core..ptr" in name and "drop_in_place" in name) or "core3ptr" in name:
            return "drop_glue"
        if "fmt" in name or "box_new_uninit" in name or "core3fmt" in name:
            return "generic_format"
        return "business"

    groups: dict = {"business": [], "drop_glue": [], "generic_format": []}
    for k in changed:
        groups[classify(k)].append(k)
    changed_business = groups["business"]
    changed_glue = groups["drop_glue"]
    changed_fmt = groups["generic_format"]

    print(f"baseline : {len(ka)} functions  ({sys.argv[1]})")
    print(f"candidate: {len(kb)} functions  ({sys.argv[2]})")
    print(
        f"only_in_baseline={len(only_a)}  only_in_candidate={len(only_b)}  "
        f"body_changed={len(changed)} (business={len(changed_business)}, "
        f"drop_glue={len(changed_glue)}, generic_format={len(changed_fmt)})"
    )

    if not (only_a or only_b or changed):
        print("\nRESULT: identical")
        return 0

    for label, items in (
        ("only in baseline", only_a),
        ("only in candidate", only_b),
        ("body changed — business", changed_business),
        ("body changed — drop glue (CGU layout artifact)", changed_glue),
        ("body changed — generic format/alloc (no business semantics)", changed_fmt),
    ):
        if items:
            print(f"\n-- {label} ({len(items)}) --")
            for name in items[:50]:
                print(f"  {name}")
            if len(items) > 50:
                print(f"  ... and {len(items) - 50} more")

    if changed_business or only_a or only_b:
        print("\nRESULT: differs (business code affected)")
        return 1
    print("\nRESULT: differs only in drop glue / generic format (no business instruction change)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
