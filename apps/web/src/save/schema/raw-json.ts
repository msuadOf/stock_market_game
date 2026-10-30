import { RawSaveNumber, SaveSchemaError, string } from "./primitives.ts";

/** 保留数值 token 到具体字段 codec；拒绝 JSON.parse 会提前覆盖的重复键。 */
export function parseRawSaveJson(raw: string): unknown {
  if (typeof raw !== "string") throw new SaveSchemaError("根节点", "必须是 JSON 字符串");
  let cursor = 0;
  const pathSegments: (string | number)[] = [];
  const renderPath = (): string => {
    let path = "根节点";
    for (const segment of pathSegments) path += typeof segment === "number" ? `[${segment}]` : `.${segment}`;
    return path;
  };
  const fail = (detail: string): never => { throw new SaveSchemaError(renderPath(), `存档不是合法 JSON：${detail}`); };
  const inheritedPropertyDescriptor: PropertyDescriptor = Object.create(null);
  inheritedPropertyDescriptor.writable = true;
  inheritedPropertyDescriptor.enumerable = true;
  inheritedPropertyDescriptor.configurable = true;
  const skip = () => {
    while (cursor < raw.length) {
      const code = raw.charCodeAt(cursor);
      if (code !== 0x20 && code !== 0x09 && code !== 0x0d && code !== 0x0a) break;
      cursor++;
    }
  };
  const quoted = (): string => {
    if (raw[cursor] !== '"') fail("JSON 对象键必须是字符串");
    const start = cursor++;
    let needsStringValidation = false;
    while (cursor < raw.length) {
      const code = raw.charCodeAt(cursor);
      const char = raw[cursor++];
      if (char === "\\") { needsStringValidation = true; cursor++; continue; }
      if (char === '"') {
        if (!needsStringValidation) return raw.slice(start + 1, cursor - 1);
        try { return string(JSON.parse(raw.slice(start, cursor)), renderPath()); }
        catch (error) {
          if (error instanceof SaveSchemaError) throw error;
          throw new SaveSchemaError(renderPath(), `包含非法 JSON 字符串：${error instanceof Error ? error.message : String(error)}`);
        }
      }
      if (code < 0x20 || (code >= 0xd800 && code <= 0xdfff)) needsStringValidation = true;
    }
    return fail("JSON 字符串未闭合");
  };
  const value = (): unknown => {
    skip(); const first = raw[cursor];
    if (first === '"') return quoted();
    if (first === "{") {
      cursor++; skip(); const result: Record<string, unknown> = {};
      if (raw[cursor] === "}") { cursor++; return result; }
      for (;;) {
        skip(); pathSegments.push("key"); const key = quoted(); pathSegments.pop();
        if (Object.hasOwn(result, key)) { pathSegments.push(key); fail("包含重复 JSON 键"); }
        skip();
        if (raw[cursor++] !== ":") fail("JSON 字段缺少冒号");
        pathSegments.push(key); const child = value(); pathSegments.pop();
        if (key in result) {
          inheritedPropertyDescriptor.value = child;
          Object.defineProperty(result, key, inheritedPropertyDescriptor);
        } else {
          result[key] = child;
        }
        skip();
        if (raw[cursor] === "}") { cursor++; return result; }
        if (raw[cursor++] !== ",") fail("JSON 对象缺少逗号或结束符");
      }
    }
    if (first === "[") {
      cursor++; skip(); const result: unknown[] = [];
      if (raw[cursor] === "]") { cursor++; return result; }
      for (;;) {
        pathSegments.push(result.length); const item = value(); pathSegments.pop(); result.push(item); skip();
        if (raw[cursor] === "]") { cursor++; return result; }
        if (raw[cursor++] !== ",") fail("JSON 数组缺少逗号或结束符");
      }
    }
    if (first === "n" && raw.startsWith("null", cursor)) { cursor += 4; return null; }
    if (first === "t" && raw.startsWith("true", cursor)) { cursor += 4; return true; }
    if (first === "f" && raw.startsWith("false", cursor)) { cursor += 5; return false; }
    const start = cursor;
    while (cursor < raw.length) {
      const code = raw.charCodeAt(cursor);
      if (code === 0x20 || code === 0x09 || code === 0x0d || code === 0x0a || code === 0x2c || code === 0x5d || code === 0x7d) break;
      cursor++;
    }
    const token = raw.slice(start, cursor);
    if (!/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?$/.test(token)) fail("包含非法 JSON 数值或值");
    return new RawSaveNumber(token);
  };
  const parsed = value(); skip();
  if (cursor !== raw.length) throw new SaveSchemaError("根节点", "存档不是合法 JSON：JSON 根值之后包含多余内容");
  return parsed;
}
