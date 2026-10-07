/* oxlint-disable react/only-export-components -- 元↔分换算 helper 供 SSR 测试与输入组件共用 */
import { useEffect, useState } from "react";

interface ParValueInputProps {
  /** 面值草稿（规范分字符串，如 "100" = 1 元/股）。 */
  readonly value: string;
  readonly onChange: (value: string) => void;
}

/** 元文本（最多两位小数）→ 规范分字符串；非元格式或零返回 null（与偏好门槛
 * 输入同一换算纪律）。 */
export function yuanTextToParCents(text: string): string | null {
  const trimmed = text.trim();
  if (!/^\d+(\.\d{1,2})?$/.test(trimmed)) return null;
  const [whole, fraction = ""] = trimmed.split(".");
  return `${whole}${(fraction + "00").slice(0, 2)}`.replace(/^0+(?=\d)/, "");
}

function centsToYuanText(cents: string): string {
  const padded = cents.replace("-", "").padStart(3, "0");
  return `${cents.startsWith("-") ? "-" : ""}${padded.slice(0, -2)}.${padded.slice(-2)}`;
}

/** 每股面值输入（2026-10-08 N2a 用户决策「面值默认 1 元/股，新局可编辑」）：
 * 界面按元录入，草稿按分存储；开局自动装配按「面值 × 总股本」推定各公司
 * 注册资本法定事实。
 *
 * 提交纪律（独立复核 minor-2 修复）：**失焦时才提交合法值**——逐击键提交会把
 * 「15 元」的中途态「1 元」写进草稿，用户中途点「创建新游戏」即用半截值；
 * 非法文本永不写草稿并显式提示。展示与草稿的差异对用户可见，不静默失同步。 */
export function ParValueInput({ value, onChange }: ParValueInputProps) {
  const [text, setText] = useState(centsToYuanText(value));
  // 外部草稿变化（读档回填）同步回输入框；本地编辑不触发回环。
  useEffect(() => {
    setText(centsToYuanText(value));
  }, [value]);
  const parsed = yuanTextToParCents(text);
  const invalid = parsed === null || parsed === "0";
  const commitOnBlur = () => {
    if (parsed !== null && parsed !== "0" && parsed !== value) onChange(parsed);
    else setText(centsToYuanText(value));
  };
  return (
    <fieldset className="par-value-input" title="仅对新游戏生效。开局自动装配按「面值 × 总股本」推定各公司注册资本法定事实。">
      <legend>每股面值</legend>
      <label>
        面值：
        <input
          type="number"
          min={0.01}
          step={0.01}
          value={text}
          onInput={(event) => setText(event.currentTarget.value)}
          onBlur={commitOnBlur}
          onKeyDown={(event) => {
            if (event.key === "Enter") commitOnBlur();
          }}
          aria-label="每股面值（元）"
          aria-invalid={invalid ? true : undefined}
        />
        元/股（{value} 分，默认 1 元；失焦生效）
        {invalid && <small role="alert">当前面值输入无效（须为正数且最多两位小数），草稿保持 {centsToYuanText(value)} 元。</small>}
        {!invalid && parsed !== value && <small>（未提交的编辑：失焦后写入草稿，将变为 {parsed} 分。）</small>}
      </label>
      <small>新局创建时按「面值 × 总股本」推定各公司注册资本；开局后随存档固化，不可修改。</small>
    </fieldset>
  );
}
