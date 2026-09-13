import { useId } from "react";
import { parseStartDate } from "./start-date.ts";

interface StartDateInputProps {
  readonly value: string;
  readonly onChange: (value: string) => void;
  readonly error: string | null;
  readonly compact?: boolean;
}

export function StartDateInput({ value, onChange, error, compact = false }: StartDateInputProps) {
  const id = useId();
  const validation = parseStartDate(value);
  const describedBy = error !== null || validation.kind === "invalid" ? `${id}-error` : undefined;
  const validationMessage = validation.kind === "invalid" ? validation.message : null;
  return (
    <label className={`start-date-input${compact ? " is-compact" : ""}`} htmlFor={id}>
      <span>模拟起始日期</span>
      <input
        id={id}
        type="date"
        min="2000-01-01"
        max="2099-12-31"
        value={value}
        aria-describedby={describedBy}
        aria-invalid={error !== null || validation.kind === "invalid"}
        onChange={(event) => onChange(event.currentTarget.value)}
      />
      <small>范围 2000-01-01 至 2099-12-31。休市日会按所选日期保留，不会自动跳转。</small>
      {(error !== null || validation.kind === "invalid") && (
        <b id={`${id}-error`} role="alert">{error ?? validationMessage}</b>
      )}
    </label>
  );
}
