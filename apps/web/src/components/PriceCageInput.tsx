interface PriceCageInputProps {
  readonly enabled: boolean;
  readonly onChange: (enabled: boolean) => void;
}

export function PriceCageInput({ enabled, onChange }: PriceCageInputProps) {
  return (
    <label className="price-cage-input" title="仅对新游戏生效。关闭价格笼子属于游戏简化，涨跌停限制仍保留。">
      <span>
        <input type="checkbox" checked={enabled} onChange={(event) => onChange(event.currentTarget.checked)} />
        新局价格笼子
      </span>
      {!enabled && <small>已关闭 · 游戏简化</small>}
    </label>
  );
}
