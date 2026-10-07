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

/** 三层税制（2026-10-08 产品决策）的新局分红税务模式选项：默认勾选简税
 *（FlatWithholding，按比例直接代扣），可选大 A 个人差别化与不扣税。
 * 仅对新游戏生效，开局后不可修改（税账只能在装配期配置）。
 * 简税比例输入：界面按百分比录入，草稿按 bp 存储；非法输入不写草稿。 */
export function TaxModeInput({ value, onChange, flatWithholdingBp, onFlatWithholdingBpChange }: TaxModeInputProps) {
  const handleRateInput = (raw: string) => {
    const parsed = Number(raw)
    if (raw.trim() === "" || !Number.isFinite(parsed)) return
    // 百分比 → bp：最多保留两位小数（1bp 粒度），超界不写草稿。
    const bp = Math.round(parsed * 100)
    if (bp < 0 || bp > MAX_FLAT_WITHHOLDING_BP || !Number.isSafeInteger(bp)) return
    onFlatWithholdingBpChange(bp)
  }
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
            type="number"
            min={0}
            max={100}
            step={0.01}
            value={flatWithholdingBp / 100}
            onInput={(event) => handleRateInput(event.currentTarget.value)}
            aria-label="简税代扣比例（百分比）"
          />
          %（{flatWithholdingBp}bp，0–100 可编辑）
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
        <small>个人（玩家与散户 NPC）按持股期限差别化计税（三档/FIFO/转让补缴）；机构/企业计税未实现，保持不计税。</small>
      )}
      {value === "Exempt" && (
        <small>不扣税模式：本局分红不产生个人股息税事实，卖出印花税也免征；佣金/过户费照付。</small>
      )}
    </fieldset>
  );
}
