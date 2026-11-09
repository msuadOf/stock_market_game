import type { QuoteCoverage } from "./stock-identity.ts";
import { ProtocolError } from "./types.ts";

/** 只校验 producer 声明的报价覆盖；不能从省略载荷重新证明其原事件类型。 */
export function validateSequenceCoverage(seqs: readonly number[], ranges: readonly QuoteCoverage[], from: number, to: number, where: string): void {
  const invalid = (): never => { throw new ProtocolError("PROTOCOL_MALFORMED", where, `事实与报价 seq 覆盖必须精确等于 (${from}, ${to}]`); };
  if (!Number.isSafeInteger(from) || !Number.isSafeInteger(to) || from < 0 || to < from) invalid();
  let previous = from;
  for (const range of ranges) {
    if (!Number.isSafeInteger(range.seq_from) || !Number.isSafeInteger(range.seq_to) || range.seq_from < previous || range.seq_to <= range.seq_from || range.seq_to > to) invalid();
    previous = range.seq_to;
  }
  const segments = [...ranges, ...seqs.map(seq => {
    if (!Number.isSafeInteger(seq) || seq <= from || seq > to) invalid();
    return { seq_from: seq - 1, seq_to: seq };
  })].sort((left, right) => left.seq_from - right.seq_from);
  previous = from;
  for (const segment of segments) {
    if (segment.seq_from !== previous) invalid();
    previous = segment.seq_to;
  }
  if (previous !== to) invalid();
}
