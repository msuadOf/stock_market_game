import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import path from 'node:path';

function checkedEvents(raw) {
  let insideString = false, escaped = false;
  for (let index = 0; index < raw.length; index++) {
    const ch = raw[index];
    if (insideString) {
      if (escaped) escaped = false;
      else if (ch === '\\') escaped = true;
      else if (ch === '"') insideString = false;
    } else if (ch === '"') insideString = true;
    else if (ch === '-' || (ch >= '0' && ch <= '9')) {
      let end = index + 1;
      while (end < raw.length && /[0-9eE+.-]/.test(raw[end])) end++;
      const token = raw.slice(index, end);
      assert.match(token, /^-?(?:0|[1-9][0-9]*)$/, '事件数字必须是整数形式');
      assert.notEqual(token, '-0', '事件负零不能无损归一');
      const exact = BigInt(token);
      assert.ok(exact >= -9007199254740991n && exact <= 9007199254740991n, '事件整数超出安全范围');
      index = end - 1;
    }
  }
  const events = JSON.parse(raw);
  assert.ok(Array.isArray(events), '事件必须是数组');
  const check = value => {
    if (typeof value === 'number') assert.ok(Number.isSafeInteger(value), '事件数字不能安全精确解析');
    else if (value && typeof value === 'object') for (const child of Object.values(value)) check(child);
  };
  for (const event of events) {
    assert.ok(event && typeof event === 'object' && !Array.isArray(event) && Object.keys(event).length === 1, '事件结构无效');
    const payload = Object.values(event)[0];
    assert.ok(payload && typeof payload === 'object' && !Array.isArray(payload), '事件内容无效');
    check(payload);
  }
  return events;
}

export function inspectDifference(difference) {
  assert.ok(Number.isSafeInteger(difference.offset) && difference.offset >= 0, '差异偏移无效');
  const left = checkedEvents(difference.baselineUtf8);
  const right = checkedEvents(difference.otherUtf8);
  let index = 0;
  while (index < Math.min(left.length, right.length) && JSON.stringify(left[index]) === JSON.stringify(right[index])) index++;
  const firstDifferentEventIndex = index;
  const withoutSeq = events => events.map(event => {
    const copy = structuredClone(event);
    delete Object.values(copy)[0].seq;
    return JSON.stringify(copy);
  }).sort();
  return {offset:difference.offset, baselineCount:left.length, otherCount:right.length, firstDifferentEventIndex,
    baselineEvent:left[index] ?? null, otherEvent:right[index] ?? null,
    equalEventMultisetIgnoringSeq:JSON.stringify(withoutSeq(left)) === JSON.stringify(withoutSeq(right))};
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 3) throw Error('必须提供恢复实验失败证据路径');
  const failure = JSON.parse(await readFile(process.argv[2], 'utf8'));
  const run = failure.observed;
  assert.ok(run && run.control && run.candidate, '缺少对照结果');
  const branches = Object.fromEntries(['control','candidate'].map(name => [name,
    run[name].firstEventDifference ? inspectDifference(run[name].firstEventDifference) : null]));
  console.log(JSON.stringify({order:run.executionOrder, saveTick:run.saveTick, ...branches}));
}
