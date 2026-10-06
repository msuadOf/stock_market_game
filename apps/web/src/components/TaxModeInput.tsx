import type { CashDividendTaxMode } from "../types/generated/CashDividendTaxMode.ts";

interface TaxModeInputProps {
  readonly value: CashDividendTaxMode;
  readonly onChange: (value: CashDividendTaxMode) => void;
}

/**
 * 新局现金分红税务模式选项（2026-10-06 产品决策）：
 * 默认勾选大 A 个人差别化；可显式选择不扣税。仅对新游戏生效，
 * 开局后不可修改（税账只能在装配期配置）。
 */
export function TaxModeInput({ value, onChange }: TaxModeInputProps) {
  return (
    <fieldset className="tax-mode-input" title="仅对新游戏生效。开局后税务模式随存档固化，不可修改。">
      <legend>新局分红税务模式</legend>
      <label>
        <input
          type="radio"
          name="tax-mode"
          value="IndividualPublicMarket"
          checked={value === "IndividualPublicMarket"}
          onChange={() => onChange("IndividualPublicMarket")}
        />
        大 A 方式（个人差别化计税，默认）
      </label>
      <label>
        <input
          type="radio"
          name="tax-mode"
          value="Exempt"
          checked={value === "Exempt"}
          onChange={() => onChange("Exempt")}
        />
        不扣税（游戏简化）
      </label>
      {value === "Exempt" && <small>不扣税模式：本局分红不产生个人股息税事实。</small>}
      {value === "IndividualPublicMarket" && (
        <small>个人（玩家与散户 NPC）按持股期限差别化计税；机构/企业计税未实现，保持不计税。</small>
      )}
    </fieldset>
  );
}
