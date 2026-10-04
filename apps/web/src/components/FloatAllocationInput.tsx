import { useEffect, useState } from "react";
import { HTMLSelect } from "@blueprintjs/core";
import type { BetweenKindDistribution, FloatAllocation, WithinKindDistribution } from "../types/engine.ts";
import "./FloatAllocationInput.css";

interface FloatAllocationInputProps {
  readonly value: FloatAllocation;
  readonly onChange: (value: FloatAllocation) => void;
}

const DEFAULT_KIND_WEIGHTS = { retail: 0.45, inst: 0.53, hot: 0.02 };

export function FloatAllocationInput({ value, onChange }: FloatAllocationInputProps) {
  const percentage = typeof value.between_kinds === "string" ? null : value.between_kinds.Percentage;

  function changeBetweenMode(mode: "Random" | "Percentage") {
    const between_kinds: BetweenKindDistribution = mode === "Random"
      ? "Random"
      : { Percentage: percentage ?? DEFAULT_KIND_WEIGHTS };
    onChange({ ...value, between_kinds });
  }

  function changeWithinMode(within_kind: WithinKindDistribution) {
    onChange({ ...value, within_kind });
  }

  function changeWeight(kind: keyof typeof DEFAULT_KIND_WEIGHTS, weight: number) {
    const weights = percentage ?? DEFAULT_KIND_WEIGHTS;
    onChange({
      ...value,
      between_kinds: { Percentage: { ...weights, [kind]: weight } },
    });
  }

  return (
    <details className="float-allocation-details">
      <summary>初始持仓分配</summary>
      <fieldset className="float-allocation-input" aria-label="新局流通盘分配">
        <legend>新局初始持仓分配</legend>
        <label>
          <span>类别间</span>
          <HTMLSelect
            aria-label="类别间分配方式"
            value={percentage === null ? "Random" : "Percentage"}
            onChange={(event) => changeBetweenMode(event.currentTarget.value as "Random" | "Percentage")}
            options={[
              { label: "随机分配", value: "Random" },
              { label: "按类别比例", value: "Percentage" },
            ]}
          />
        </label>
        {percentage !== null && <div className="float-allocation-weights">
          <PercentageInput label="散户" value={percentage.retail} onChange={(weight) => changeWeight("retail", weight)} />
          <PercentageInput label="机构" value={percentage.inst} onChange={(weight) => changeWeight("inst", weight)} />
          <PercentageInput label="游资" value={percentage.hot} onChange={(weight) => changeWeight("hot", weight)} />
        </div>}
        <label>
          <span>类别内</span>
          <HTMLSelect
            aria-label="类别内分配方式"
            value={value.within_kind}
            onChange={(event) => changeWithinMode(event.currentTarget.value as WithinKindDistribution)}
            options={[
              { label: "等比例分配", value: "EqualPercentage" },
              { label: "随机分配", value: "Random" },
            ]}
          />
        </label>
        <small>类别权重按总和归一；等比例分配时余股按账户顺序分配，缺少的类别会重新归一。</small>
      </fieldset>
    </details>
  );
}

interface PercentageInputProps {
  readonly label: string;
  readonly value: number;
  readonly onChange: (value: number) => void;
}

function PercentageInput({ label, value, onChange }: PercentageInputProps) {
  const [draft, setDraft] = useState(String(value * 100));
  const [error, setError] = useState<string | null>(null);

  useEffect(() => setDraft(String(value * 100)), [value]);

  function commitDraft() {
    const parsed = Number(draft);
    if (!draft.trim() || !Number.isFinite(parsed) || parsed < 0) {
      setError("请输入大于或等于 0 的有限百分比");
      onChange(Number.NaN);
      return;
    }
    setError(null);
    onChange(parsed / 100);
  }

  return (
    <label>
      <span>{label} (%)</span>
      <input
        aria-label={`${label}类别占比百分数`}
        aria-invalid={error !== null}
        type="number"
        min="0"
        step="0.1"
        value={draft}
        onChange={(event) => setDraft(event.currentTarget.value)}
        onBlur={commitDraft}
      />
      {error !== null && <small role="alert">{error}</small>}
    </label>
  );
}
