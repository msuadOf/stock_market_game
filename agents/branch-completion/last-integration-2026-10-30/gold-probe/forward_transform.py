"""本次旧新 producer 取证：只按冻结源定义前向生成 expected，不读 actual 决定 expected。"""

import hashlib
import json
import sys
from concurrent.futures import ProcessPoolExecutor
from dataclasses import dataclass
from pathlib import Path


OLD_ANCHORS = {
    0: 16039080022122580669,
    3: 7734365835074963189,
    6: 6825886869182669854,
}
OLD_SOURCE_REVISION = "884bd5ad12d71608ff7db1524292c98b96dc2978"
OLD_RECONSTRUCTION_REVISION = "fbf31a58d014be22972ce609cf0976f8cf1c76f6"
CURRENT_SOURCE_REVISION = "c91273a684eb623b9c40043c3d1ff1bf8d5c2b88"
DECODER = json.JSONDecoder()


@dataclass
class Node:
    text: str
    start: int
    end: int
    value: object
    members: list
    items: list

    def raw(self):
        return self.text[self.start:self.end]

    def field(self, key):
        matches = [node for name, _, node in self.members if name == key]
        assert len(matches) == 1, f"字段必须唯一存在：{key}"
        return matches[0]

    def has(self, key):
        return any(name == key for name, _, _ in self.members)

    def compact_members(self):
        assert isinstance(self.value, dict), "要求 object"
        raw_members = [self.text[start:node.end] for _, start, node in self.members]
        assert "{" + ",".join(raw_members) + "}" == self.raw(), "原 object 必须是未改写的 compact serde 输出"
        return raw_members


def parse_node(text, start=0):
    value, end = DECODER.raw_decode(text, start)
    node = Node(text, start, end, value, [], [])
    if isinstance(value, dict):
        cursor = start + 1
        while cursor < end - 1:
            key_start = cursor
            key, cursor = DECODER.raw_decode(text, cursor)
            assert isinstance(key, str) and text[cursor] == ":"
            child = parse_node(text, cursor + 1)
            node.members.append((key, key_start, child))
            cursor = child.end
            if cursor < end - 1:
                assert text[cursor] == ","
                cursor += 1
        assert len(node.members) == len(value), "禁止重复 object key"
    elif isinstance(value, list):
        cursor = start + 1
        while cursor < end - 1:
            child = parse_node(text, cursor)
            node.items.append(child)
            cursor = child.end
            if cursor < end - 1:
                assert text[cursor] == ","
                cursor += 1
    return node


def fnv(bytes_):
    digest = 0xCBF29CE484222325
    for byte in bytes_:
        digest = ((digest ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return digest


def sha(bytes_):
    return hashlib.sha256(bytes_).hexdigest()


def expected_phase(auction_ticks, tick):
    day_tick = tick % 20
    if day_tick < auction_ticks - auction_ticks // 3:
        return "CallAuction"
    if day_tick < auction_ticks:
        return "PreOpen"
    if day_tick >= 18:
        return "ClosingAuction"
    return "Continuous"


def forward(old_bytes, auction_ticks, tick):
    text = old_bytes.decode("utf-8")
    root = parse_node(text)
    assert root.end == len(text) and len(root.items) == 3
    events, live, save = root.items
    assert live.field("tick").value == tick
    assert save.field("snapshot").field("tick").value == tick
    assert not live.has("mirror_origin")
    origin = {
        "authority_epoch": 1,
        "install_sequence": tick + 1,
        "source_tick": tick,
        "source_generation": tick + int(tick == 20),
        "day_end_stage": "retained_published" if tick == 20 else "tick",
        "non_tick_revision": 0,
        "mirror_phase": expected_phase(auction_ticks, tick),
    }
    assert live.field("phase").value == origin["mirror_phase"]
    live.compact_members()
    live_members = []
    for key, start, child in live.members:
        live_members.append(text[start:child.end])
        if key == "phase":
            live_members.append('"mirror_origin":' + json.dumps(origin, separators=(",", ":")))
    expected_live = "{" + ",".join(live_members) + "}"

    setup = save.field("setup")
    setup.compact_members()
    assert not setup.has("runtime_sync_policy")
    expected_setup = '{"runtime_sync_policy":"every_tick",' + setup.raw()[1:]
    assert not save.has("company_state") and not save.has("report_correction_operations")
    operations = save.field("company_operations").raw()
    closing = save.field("closing_registry").raw()
    groups = save.field("groups").raw()
    company_state = (
        '{"LegacyOperations":{"operations":' + operations
        + ',"closing":' + closing + ',"groups":' + groups + "}}"
    )
    # 仅此零 NPC、未创建任何计划的 fixture；非空计划或已消费游标不可借此转换。
    plans = save.field("plans")
    plans.compact_members()
    assert [key for key, _, _ in plans.members] == ["policy", "next_plan_seq", "plans"]
    assert plans.field("next_plan_seq").raw() == "0"
    assert plans.field("plans").raw() == "{}"
    expected_plans = '{"policy":' + plans.field("policy").raw() + ',"owner_sequences":[],"plans":[]}'
    save.compact_members()
    save_members = []
    for key, start, child in save.members:
        if key in ("groups", "closing_registry"):
            continue
        if key == "setup":
            save_members.append('"setup":' + expected_setup)
        elif key == "company_operations":
            save_members.append('"company_state":' + company_state)
            save_members.append('"report_correction_operations":{}')
        elif key == "plans":
            save_members.append('"plans":' + expected_plans)
        else:
            save_members.append(text[start:child.end])
    expected_save = "{" + ",".join(save_members) + "}"
    return ("[" + events.raw() + "," + expected_live + "," + expected_save + "]").encode("utf-8")


def differences(expected, actual, path="$"):
    if type(expected) is not type(actual):
        return [{"path": path, "expected": expected, "actual": actual}]
    if isinstance(expected, dict):
        result = []
        for key in sorted(expected.keys() | actual.keys()):
            if key not in expected or key not in actual:
                result.append({"path": path + "." + key, "expected_present": key in expected,
                               "actual_present": key in actual})
            else:
                result.extend(differences(expected[key], actual[key], path + "." + key))
        return result
    if isinstance(expected, list):
        if len(expected) != len(actual):
            return [{"path": path, "expected_length": len(expected), "actual_length": len(actual)}]
        return [difference for index, (left, right) in enumerate(zip(expected, actual))
                for difference in differences(left, right, f"{path}[{index}]")]
    return [] if expected == actual else [{"path": path, "expected": expected, "actual": actual}]


def verify_old(inputs):
    old_dir, auction_ticks = inputs
    rows = [(old_dir / f"auction-{auction_ticks}-tick-{tick:02}.json").read_bytes()
            for tick in range(1, 21)]
    combined = b"".join(rows)
    assert combined == (old_dir / f"auction-{auction_ticks}-raw.bin").read_bytes()
    actual_digest = fnv(combined)
    expected_digest = OLD_ANCHORS[auction_ticks]
    assert actual_digest == expected_digest, f"旧 producer 未复现：{auction_ticks} {actual_digest} != {expected_digest}"
    return {"auction_ticks": auction_ticks, "fnv": str(actual_digest), "sha256": sha(combined)}


def compare_scenario(inputs):
    old_dir, current_dir, output, verified = inputs
    auction_ticks = verified["auction_ticks"]
    old_rows = [(old_dir / f"auction-{auction_ticks}-tick-{tick:02}.json").read_bytes()
                for tick in range(1, 21)]
    # 所有 expected 先仅由旧 bytes 和上方固定 source 公式生成，再读取本 scenario actual。
    expected_rows = [forward(row, auction_ticks, tick) for tick, row in enumerate(old_rows, 1)]
    actual_rows = [(current_dir / f"auction-{auction_ticks}-tick-{tick:02}.json").read_bytes()
                   for tick in range(1, 21)]
    assert b"".join(actual_rows) == (current_dir / f"auction-{auction_ticks}-raw.bin").read_bytes()
    scenario = {
        "auction_ticks": auction_ticks,
        "old_fnv": verified["fnv"],
        "old_sha256": verified["sha256"],
        "expected_fnv": str(fnv(b"".join(expected_rows))),
        "expected_sha256": sha(b"".join(expected_rows)),
        "actual_fnv": str(fnv(b"".join(actual_rows))),
        "actual_sha256": sha(b"".join(actual_rows)),
        "ticks": [],
    }
    all_equal = True
    for tick, (old, expected, actual) in enumerate(zip(old_rows, expected_rows, actual_rows), 1):
        same = expected == actual
        all_equal &= same
        (output / f"auction-{auction_ticks}-tick-{tick:02}-expected.json").write_bytes(expected)
        scenario["ticks"].append({
            "tick": tick, "old_sha256": sha(old), "expected_sha256": sha(expected),
            "actual_sha256": sha(actual), "byte_equal": same,
            "differences": differences(json.loads(expected), json.loads(actual)) if not same else [],
        })
    return scenario, all_equal


def main():
    assert len(sys.argv) == 4, "参数：old 原字节目录、current 原字节目录、全新派生证据目录"
    old_dir, current_dir, output = map(Path, sys.argv[1:])
    with ProcessPoolExecutor(max_workers=3) as pool:
        # 必须先核实全部旧三锚；禁止读取 current 后反推旧 expected 或扩大许可 path。
        verified = list(pool.map(verify_old, [(old_dir, auction) for auction in OLD_ANCHORS]))
        output.mkdir(parents=True, exist_ok=False)
        results = list(pool.map(compare_scenario, [
            (old_dir, current_dir, output, scenario) for scenario in verified
        ]))
        manifest = {
            "status": "FORWARD_TRANSFORM_PENDING_COMPARISON",
            "old_source_revision": OLD_SOURCE_REVISION,
            "old_reconstruction_revision": OLD_RECONSTRUCTION_REVISION,
            "current_source_revision": CURRENT_SOURCE_REVISION,
            "transform_source_sha256": sha(Path(__file__).read_bytes()),
            "old_dir": str(old_dir.resolve()),
            "current_dir": str(current_dir.resolve()),
            "workers": 3,
            "scenarios": [scenario for scenario, _ in results],
        }
    all_equal = all(equal for _, equal in results)
    manifest["status"] = "FORWARD_TRANSFORM_EXACT_60_TUPLES" if all_equal else "FORWARD_TRANSFORM_MISMATCH"
    (output / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(manifest, ensure_ascii=False, indent=2))
    return 0 if all_equal else 1


if __name__ == "__main__":
    sys.exit(main())
