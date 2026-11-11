/** 验证阶段的 LibOS 共享故障布局；地址由所属 WASM 实例固定持有。 */
export interface LibosHostFaultLayout {
  readonly globalStateAddress: number;
  readonly domainAbortAddress: number;
  readonly hostReadyAddress: number;
  readonly slots: readonly { readonly abortAddress: number; readonly waitAddress: number }[];
}
const owners = new WeakMap<LibosHostFaultLayout, { readonly memory: WebAssembly.Memory; readonly ranges: readonly { readonly start: number; readonly end: number }[] }>();

function record(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("LibOS 故障布局必须为对象");
  const object = value as Record<string, unknown>;
  if (Object.keys(object).length !== keys.length || keys.some(key => !Object.hasOwn(object, key))) throw new Error("LibOS 故障布局字段不匹配");
  return object;
}

export function parseLibosHostFaultLayout(value: unknown, memory: WebAssembly.Memory, context: { readonly capacity: number; readonly protectedRanges: { readonly arena: { readonly start: number; readonly length: number }; readonly engine: { readonly start: number; readonly length: number }; readonly ingress: { readonly start: number; readonly length: number }; readonly death: { readonly start: number; readonly length: number } } }): LibosHostFaultLayout {
  if (typeof SharedArrayBuffer === "undefined" || !(memory instanceof WebAssembly.Memory)) throw new Error("LibOS 故障布局需要共享 memory");
  const buffer: unknown = memory.buffer;
  if (!(buffer instanceof SharedArrayBuffer)) throw new Error("LibOS 故障布局需要共享 memory");
  const size = buffer.byteLength;
  const ranges: Array<{start: number; end: number}> = [];
  const address = (raw: unknown, bytes: number): number => {
    if (typeof raw !== "number" || !Number.isInteger(raw) || raw <= 0 || raw > 0xffff_ffff || raw % bytes !== 0 || raw + bytes > size) throw new Error("LibOS 故障地址无效或越界");
    ranges.push({start: raw, end: raw + bytes});
    return raw;
  };
  const root = record(value, ["globalStateAddress", "domainAbortAddress", "hostReadyAddress", "slots"]);
  const globalStateAddress = address(root.globalStateAddress, 4);
  const domainAbortAddress = address(root.domainAbortAddress, 1);
  const hostReadyAddress = address(root.hostReadyAddress, 4);
  if (!Number.isInteger(context.capacity) || context.capacity <= 0 || context.capacity > 2 ** 20 || !Array.isArray(root.slots) || root.slots.length !== context.capacity) throw new Error("LibOS 故障域槽数与 facade 不一致");
  const slots = Array.from(root.slots, slot => {
    const item = record(slot, ["abortAddress", "waitAddress"]);
    return Object.freeze({abortAddress: address(item.abortAddress, 1), waitAddress: address(item.waitAddress, 4)});
  });
  ranges.sort((a, b) => a.start - b.start);
  for (let index = 1; index < ranges.length; index++) {
    if (ranges[index].start < ranges[index - 1].end) throw new Error("LibOS 故障地址重叠");
  }
  const protectedRegions = record(context.protectedRanges, ["arena", "engine", "ingress", "death"]);
  for (const value of Object.values(protectedRegions)) {
    const region = record(value, ["start", "length"]);
    const { start, length } = region;
    if (typeof start !== "number" || typeof length !== "number" || !Number.isSafeInteger(start) || !Number.isSafeInteger(length) || start < 0 || length <= 0 || start + length > size) throw new Error("LibOS 保护区域无效");
    if (ranges.some(range => range.start < start + length && start < range.end)) throw new Error("LibOS 故障地址与受保护区域重叠");
  }
  if (Atomics.load(new Int32Array(memory.buffer, globalStateAddress, 1), 0) !== 1 || Atomics.load(new Uint8Array(memory.buffer, domainAbortAddress, 1), 0) !== 0 || Atomics.load(new Int32Array(memory.buffer, hostReadyAddress, 1), 0) !== 0) throw new Error("LibOS 故障域尚未就绪或已经故障");
  const result = Object.freeze({globalStateAddress, domainAbortAddress, hostReadyAddress, slots: Object.freeze(slots)});
  owners.set(result, { memory, ranges });
  return result;
}

/** 永久发布宿主故障；先设域状态，再唤醒所有登记槽。 */
export function abortLibosHostFault(memory: WebAssembly.Memory, layout: LibosHostFaultLayout): void {
  if (owners.get(layout)?.memory !== memory) throw new Error("LibOS 故障布局不属于此 memory");
  const errors: unknown[] = [];
  function attempt(action: () => void): void {
    try { action(); } catch (error) { errors.push(error); }
  }
  attempt(() => { Atomics.or(new Int32Array(memory.buffer, layout.globalStateAddress, 1), 0, 2); });
  attempt(() => { Atomics.store(new Uint8Array(memory.buffer, layout.domainAbortAddress, 1), 0, 1); });
  for (const slot of layout.slots) {
    attempt(() => { Atomics.store(new Uint8Array(memory.buffer, slot.abortAddress, 1), 0, 1); });
    const word = new Int32Array(memory.buffer, slot.waitAddress, 1);
    attempt(() => { Atomics.exchange(word, 0, 1); });
    attempt(() => { Atomics.notify(word, 0); });
  }
  if (errors.length !== 0) throw new AggregateError(errors, "LibOS 故障状态已尽力发布，但部分等待者通知失败", { cause: errors });
}

export function overlapsLibosHostFault(layout: LibosHostFaultLayout, start: number, length: number): boolean {
  const owner = owners.get(layout);
  if (owner === undefined || !Number.isSafeInteger(start) || !Number.isSafeInteger(length) || start < 0 || length <= 0 || start + length > owner.memory.buffer.byteLength) throw new Error("LibOS 重叠检查范围非法");
  let low = 0, high = owner.ranges.length;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if (owner.ranges[middle].end <= start) low = middle + 1;
    else high = middle;
  }
  return low < owner.ranges.length && owner.ranges[low].start < start + length;
}

export function activateLibosHostFault(memory: WebAssembly.Memory, layout: LibosHostFaultLayout): void {
  if (owners.get(layout)?.memory !== memory) throw new Error("LibOS 故障布局不属于此 memory");
  const buffer = memory.buffer;
  const global = new Int32Array(buffer, layout.globalStateAddress, 1);
  const aborted = new Uint8Array(buffer, layout.domainAbortAddress, 1);
  const ready = new Int32Array(buffer, layout.hostReadyAddress, 1);
  if (Atomics.load(global, 0) !== 1 || Atomics.load(aborted, 0) !== 0) throw new Error("LibOS 故障域未就绪或已故障");
  if (Atomics.compareExchange(ready, 0, 0, 1) !== 0) throw new Error("LibOS host-ready 已发布");
  if (Atomics.load(global, 0) !== 1 || Atomics.load(aborted, 0) !== 0) throw new Error("LibOS host-ready 发布时发生故障");
}
