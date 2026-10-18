import { MOBILE_SPEED_OPTIONS, mobileSpeedLabel } from "./mobile-ui-state";
import "./MobileSpeedSelect.css";

interface Props {
  speed: number;
  measuredSpeed?: string;
  measuredSpeedTitle?: string;
  onChange: (speed: number) => void;
}

export function MobileSpeedSelect({ speed, measuredSpeed, measuredSpeedTitle, onChange }: Props) {
  return (
    <span className={`mobile-speed-select-wrap ${measuredSpeed ? "has-telemetry" : ""}`}>
      <select aria-label="模拟速度" value={String(speed)} onChange={(event) => onChange(Number(event.target.value))}>
        {!MOBILE_SPEED_OPTIONS.some((value) => value === speed) && <option value={String(speed)}>{mobileSpeedLabel(speed)}</option>}
        {MOBILE_SPEED_OPTIONS.map((value) => <option key={String(value)} value={String(value)}>{mobileSpeedLabel(value)}</option>)}
      </select>
      <span className="mobile-speed-chevron" aria-hidden="true">⌄</span>
      {measuredSpeed && (
        <output className="mobile-speed-actual" aria-label={measuredSpeed} title={measuredSpeedTitle}>
          {measuredSpeed}
        </output>
      )}
    </span>
  );
}
