import { RawSaveNumber, SaveSchemaError, string } from "./primitives.ts";

/** 保留数值 token 到具体字段 codec；拒绝 JSON.parse 会提前覆盖的重复键。 */
export function parseRawSaveJson(raw: string): unknown {
  if (typeof raw !== "string") throw new SaveSchemaError("根节点", "必须是 JSON 字符串");
  let cursor = 0;
  const fail = (path: string, detail: string): never => { throw new SaveSchemaError(path, `存档不是合法 JSON：${detail}`); };
  const skip = () => { while (cursor < raw.length && /[ \t\r\n]/.test(raw[cursor])) cursor++; };
  const quoted = (path: string): string => {
    if (raw[cursor] !== '"') fail(path, "JSON 对象键必须是字符串");
    const start = cursor++;
    while (cursor < raw.length) {
      const char = raw[cursor++];
      if (char === "\\") { cursor++; continue; }
      if (char === '"') {
        try { return string(JSON.parse(raw.slice(start, cursor)), path); }
        catch (error) {
          if (error instanceof SaveSchemaError) throw error;
          throw new SaveSchemaError(path, `包含非法 JSON 字符串：${error instanceof Error ? error.message : String(error)}`);
        }
      }
    }
    return fail(path, "JSON 字符串未闭合");
  };
  const value = (path: string): unknown => {
    skip(); const first = raw[cursor];
    if (first === '"') return quoted(path);
    if (first === "{") {
      cursor++; skip(); const entries: [string, unknown][] = [], seen = new Set<string>();
      if (raw[cursor] === "}") { cursor++; return {}; }
      for (;;) {
        skip(); const key = quoted(`${path}.key`);
        if (seen.has(key)) fail(`${path}.${key}`, "包含重复 JSON 键");
        seen.add(key); skip();
        if (raw[cursor++] !== ":") fail(path, "JSON 字段缺少冒号");
        entries.push([key, value(`${path}.${key}`)]); skip();
        if (raw[cursor] === "}") { cursor++; return Object.fromEntries(entries); }
        if (raw[cursor++] !== ",") fail(path, "JSON 对象缺少逗号或结束符");
      }
    }
    if (first === "[") {
      cursor++; skip(); const result: unknown[] = [];
      if (raw[cursor] === "]") { cursor++; return result; }
      for (;;) {
        result.push(value(`${path}[${result.length}]`)); skip();
        if (raw[cursor] === "]") { cursor++; return result; }
        if (raw[cursor++] !== ",") fail(path, "JSON 数组缺少逗号或结束符");
      }
    }
    for (const [token, decoded] of [["null", null], ["true", true], ["false", false]] as const) {
      if (raw.startsWith(token, cursor)) { cursor += token.length; return decoded; }
    }
    const start = cursor;
    while (cursor < raw.length && !/[ \t\r\n,\]}]/.test(raw[cursor])) cursor++;
    const token = raw.slice(start, cursor);
    if (!/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?$/.test(token)) fail(path, "包含非法 JSON 数值或值");
    return new RawSaveNumber(token);
  };
  const parsed = value("根节点"); skip();
  if (cursor !== raw.length) fail("根节点", "JSON 根值之后包含多余内容");
  return parsed;
}
