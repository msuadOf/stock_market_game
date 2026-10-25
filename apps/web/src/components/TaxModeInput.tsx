import { useEffect, useState } from "react";
import type { CashDividendTaxMode } from "../types/generated/CashDividendTaxMode.ts";

interface TaxModeInputProps {
  readonly value: CashDividendTaxMode;
  readonly onChange: (value: CashDividendTaxMode) => void;
  /** 简税比例草稿（bp）；仅 Flat 模式使用。 */
  readonly flatWithholdingBp: number;
  /** 简税比例草稿写入（bp）；非法输入不写入并提示。 */
  readonly onFlatWithholdingBpChange: (value: number) => void;
}

/** 简税比例输入框的合法域：0..=10000bp（最高全额代扣，与引擎契约一致）。 */
export const MAX_FLAT_WITHHOLDING_BP = 10000;

/** 先用十进制原文校验 0–100%，再按 1bp 四舍五入，避免浮点近似改变边界。
 * 科学记数法沿用原数字输入的合法形式；指数只移动小数点，不参与金额近似。 */
function percentToFlatWithholdingBp(raw: string): number | null {
  const match = /^([+-]?)(\d*(?:\.\d*)?)(?:[eE]([+-]?\d+))?$/.exec(raw.trim());
  if (match === null) return null;
  const [, sign, decimal, exponent = "0"] = match;
  const [whole, fraction = ""] = decimal!.split(".");
  const combined = whole! + fraction;
  if (combined.length === 0) return null;
  const digits = combined.replace(/^0+/, "");
  if (digits.length === 0) return 0;
  if (sign === "-") return null;
  const point = whole!.length + Number(exponent) - (combined.length - digits.length);
  if (point > 3) return null;
  if (point === 3 && (digits.slice(0, 3).padEnd(3, "0") !== "100" || /[1-9]/.test(digits.slice(3)))) return null;
  if (point < -2) return 0;
  const bpPoint = point + 2;
  const integerBp = bpPoint === 0 ? 0 : Number(digits.slice(0, bpPoint).padEnd(bpPoint, "0"));
  return integerBp + (digits[bpPoint] !== undefined && digits[bpPoint]! >= "5" ? 1 : 0);
}

/** ADR-0040 的三层税务模式；百分比文本经域校验后按 1bp 舍入写入草稿。
 * 非法输入保留原文并显示已设比例，外部草稿变化通过 effect 同步。 */
export function TaxModeInput({ value, onChange, flatWithholdingBp, onFlatWithholdingBpChange }: TaxModeInputProps) {
  const [rateText, setRateText] = useState((flatWithholdingBp / 100).toString());
  useEffect(() => {
    setRateText((flatWithholdingBp / 100).toString());
  }, [flatWithholdingBp]);
  const handleRateInput = (raw: string) => {
    setRateText(raw);
    const bp = percentToFlatWithholdingBp(raw);
    if (bp === null) return;
    onFlatWithholdingBpChange(bp);
  };
  const parsedDraftBp = percentToFlatWithholdingBp(rateText);
  const rateInvalid = value === "FlatWithholding"
    && (parsedDraftBp === null || parsedDraftBp !== flatWithholdingBp);
  return (
    <fieldset className="tax-mode-input" title="仅对新游戏生效。开局后税务模式随存档固化，不可修改。">
      <legend>新局分红税务模式</legend>
      <label>
        <input
          type="radio"
          name="tax-mode"
          value="FlatWithholding"
          checked={value === "FlatWithholding"}
          onChange={() => onChange("FlatWithholding")}
        />
        简税（分红到账时按比例直接扣，默认）
      </label>
      {value === "FlatWithholding" && (
        <label>
          代扣比例：
          <input
            type="text"
            inputMode="decimal"
            value={rateText}
            onInput={(event) => handleRateInput(event.currentTarget.value)}
            aria-label="简税代扣比例（百分比）"
            aria-invalid={rateInvalid}
          />
          %（0–100%，按 0.01% 精度保存）
          {rateInvalid && <small role="alert">当前比例输入无效，请填写 0–100%；已设比例仍为 {flatWithholdingBp / 100}%。</small>}
        </label>
      )}
      <label>
        <input
          type="radio"
          name="tax-mode"
          value="AShareIndividual"
          checked={value === "AShareIndividual"}
          onChange={() => onChange("AShareIndividual")}
        />
        大 A 方式（个人差别化计税）
      </label>
      <label>
        <input
          type="radio"
          name="tax-mode"
          value="Exempt"
          checked={value === "Exempt"}
          onChange={() => onChange("Exempt")}
        />
        不扣税（连印花税也免）
      </label>
      {value === "FlatWithholding" && (
        <small>简税模式：分红付款日对每位持有人（含机构）按比例代扣；无持股期档位、卖出不补税。</small>
      )}
      {value === "AShareIndividual" && (
        <small>个人（玩家与散户 NPC）按持股期限差别化计税，卖出时扣收应缴税款；机构/企业计税未实现，暂不计税。</small>
      )}
      {value === "Exempt" && (
        <small>不扣税模式：本局分红免征个人股息税，卖出印花税也免征；佣金/过户费照付。</small>
      )}
    </fieldset>
  );
}
