/* oxlint-disable react/only-export-components -- 元↔分换算 helper 与输入组件共用 */
import { useEffect, useState } from "react";
import { centsToYuanText } from "../utils/money.ts";

interface ParValueInputProps {
  /** 面值草稿（Money 规范分字符串，如 "100" = 1 元/股）。 */
  readonly value: string;
  readonly onChange: (value: string) => void;
}

/** 元文本精确换算为正 Money 分字符串；非法格式、零和超出 i64 范围均返回 null。 */
export function yuanTextToParCents(text: string): string | null {
  const trimmed = text.trim();
  if (!/^\d+(\.\d{1,2})?$/.test(trimmed)) return null;
  const [whole, fraction = ""] = trimmed.split(".");
  const cents = `${whole}${(fraction + "00").slice(0, 2)}`.replace(/^0+(?=\d)/, "");
  if (cents === "0" || cents.length > 19 || BigInt(cents) > 9_223_372_036_854_775_807n) return null;
  return cents;
}

/** 面值以元编辑，失焦或 Enter 时提交 Money 分字符串；非法输入保留并显式提示。
 * 面值默认 1 元/股、新局可编辑，属于 2026-10-08 N2a 的游戏装配约定。 */
export function ParValueInput({ value, onChange }: ParValueInputProps) {
  const [text, setText] = useState(centsToYuanText(value));
  useEffect(() => {
    setText(centsToYuanText(value));
  }, [value]);
  const parsed = yuanTextToParCents(text);
  const invalid = parsed === null;
  const commit = () => {
    if (parsed !== null && parsed !== value) onChange(parsed);
  };
  return (
    <fieldset className="par-value-input" title="仅对新游戏生效。注册资本按「面值 × 总股本」设定。">
      <legend>每股面值</legend>
      <label>
        面值：
        <input
          type="text"
          inputMode="decimal"
          value={text}
          onInput={(event) => setText(event.currentTarget.value)}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === "Enter") commit();
          }}
          aria-label="每股面值（元）"
          aria-invalid={invalid ? true : undefined}
        />
        元/股（失焦或按 Enter 生效）
        {invalid && <small role="alert">当前面值输入无效：请输入正数，最多两位小数且不超过 92233720368547758.07 元；已设面值仍为 {centsToYuanText(value)} 元。</small>}
        {!invalid && parsed !== value && <small>当前编辑尚未生效；失焦或按 Enter 后，面值将设为 {centsToYuanText(parsed)} 元/股。</small>}
      </label>
      <small>新局创建时按「面值 × 总股本」设定各公司注册资本；开局后随存档固定，不可修改。</small>
    </fieldset>
  );
}
