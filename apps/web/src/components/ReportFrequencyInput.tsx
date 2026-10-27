import { useEffect, useRef, useState } from "react";
import type { ReportFrequency } from "../types/generated/ReportFrequency";
import type { MonthlyReportDelay } from "../types/generated/MonthlyReportDelay";
import { parseMonthlyReportSchedule } from "../save/schema/monthly-report-schedule.ts";

export type ReportFrequencyDraft = ReportFrequency | "Monthly";

export function ReportFrequencyInput({ value, onChange }: {
  readonly value: ReportFrequencyDraft;
  readonly onChange: (value: ReportFrequencyDraft) => void;
}) {
  const monthly = typeof value === "object" || value === "Monthly";
  return <fieldset><label>财报公开频率 <select
    aria-label="财报公开频率"
    value={monthly ? "Monthly" : "Quarterly"}
    onChange={(event) => {
      const frequency = event.target.value;
      if (frequency !== "Quarterly" && frequency !== "Monthly") throw new Error(`无效财报频率：${frequency}`);
      onChange(frequency);
    }}
  ><option value="Quarterly">季度（默认定期报告）</option><option value="Monthly">月度（额外游戏月报）</option></select></label>
    <p>月报是额外游戏报告，不替代法定年报、中报和季度报告；税务核算仍按年度。</p>
    {monthly && <MonthlyScheduleInput value={value} onChange={onChange} />}
  </fieldset>;
}

function MonthlyScheduleInput({ value, onChange }: { readonly value: ReportFrequencyDraft; readonly onChange: (value: ReportFrequencyDraft) => void }) {
  const schedule = typeof value === "object" ? value.Monthly.schedule : null;
  const [selection, setSelection] = useState(() => schedule === null ? "" : "Preset" in schedule ? schedule.Preset.preset : "Custom");
  const [day, setDay] = useState(() => schedule !== null && "Custom" in schedule ? String(schedule.Custom.day) : "");
  const [time, setTime] = useState(() => {
    if (schedule === null || !("Custom" in schedule)) return "";
    const second = schedule.Custom.second_of_day;
    return `${String(Math.floor(second / 3600)).padStart(2, "0")}:${String(Math.floor(second % 3600 / 60)).padStart(2, "0")}:${String(second % 60).padStart(2, "0")}`;
  });
  const initialDelay = schedule === null ? "None" : "Preset" in schedule ? schedule.Preset.delay : schedule.Custom.delay;
  const [delay, setDelay] = useState(() => initialDelay === "None" ? "0" : String(initialDelay.Uniform.max_days));
  const lastEmitted = useRef(value);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    // 本组件发出的未完成草稿保留用户原文；外部 value 替换则完整同步。
    if (value === lastEmitted.current) return;
    lastEmitted.current = value;
    setSelection(schedule === null ? "" : "Preset" in schedule ? schedule.Preset.preset : "Custom");
    const nextDelay = schedule === null ? "None" : "Preset" in schedule ? schedule.Preset.delay : schedule.Custom.delay;
    setDelay(nextDelay === "None" ? "0" : String(nextDelay.Uniform.max_days));
    setDay(schedule !== null && "Custom" in schedule ? String(schedule.Custom.day) : "");
    if (schedule !== null && "Custom" in schedule) {
      const second = schedule.Custom.second_of_day;
      setTime(`${String(Math.floor(second / 3600)).padStart(2, "0")}:${String(Math.floor(second % 3600 / 60)).padStart(2, "0")}:${String(second % 60).padStart(2, "0")}`);
    } else setTime("");
    setError(null);
  }, [value, schedule]);
  const publish = (next: ReportFrequencyDraft) => {
    lastEmitted.current = next;
    onChange(next);
  };
  const apply = (selected: string, selectedDay: string, selectedTime: string, selectedDelay: string) => {
    let next: ReportFrequencyDraft = "Monthly";
    let failureMessage: string | null = null;
    try {
      if (!["", "FirstDayEvening", "TenthDayEvening", "Custom"].includes(selected)) throw new Error(`月报排期无效：${selected}，请重新选择排期`);
      if (!/^(?:0|[1-9]\d*)$/.test(selectedDelay) || Number(selectedDelay) > 31) throw new Error("月报随机延迟必须为不延迟或1–31日的整数上限，请重新选择");
      const parsedDelay: MonthlyReportDelay = selectedDelay === "0" ? "None" : { Uniform: { max_days: Number(selectedDelay) } };
      const matches = /^(\d{2}):(\d{2})(?::(\d{2}))?$/.exec(selectedTime);
      if (selected === "Custom") {
        if (selectedDay !== "" && (!/^[1-9]\d?$/.test(selectedDay) || Number(selectedDay) > 28)) throw new Error("月报日期必须为次月1–28日整数，不自动截断；请修改日期");
        if (selectedTime !== "" && (matches === null || Number(matches[1]) > 23 || Number(matches[2]) > 59 || (matches[3] !== undefined && Number(matches[3]) > 59))) throw new Error("月报时间必须为00:00:00–23:59:59，不自动截断；请修改时间");
      }
      if (selected !== "" && !(selected === "Custom" && (selectedDay === "" || selectedTime === ""))) {
        const input = selected === "Custom" ? { Custom: { day: Number(selectedDay), second_of_day: Number(matches![1]) * 3600 + Number(matches![2]) * 60 + (matches![3] === undefined ? 0 : Number(matches![3])), delay: parsedDelay } } : { Preset: { preset: selected, delay: parsedDelay } };
        next = { Monthly: { schedule: parseMonthlyReportSchedule(input, "月报设置") } };
      }
    } catch (failure) { failureMessage = failure instanceof Error ? failure.message : String(failure); }
    setError(failureMessage);
    publish(next);
  };
  return <div>
    <label>月报排期<select aria-label="月报排期" value={selection} onChange={(event) => { const selected = event.currentTarget.value; setSelection(selected); apply(selected, day, time, delay); }}>
      <option value="">请明确选择月报排期</option><option value="FirstDayEvening">次月1日18:00</option><option value="TenthDayEvening">次月10日18:00</option><option value="Custom">自定义次月日期／时间</option>
    </select></label>
    {selection === "Custom" && <><label>次月日期（1–28）<input aria-label="月报次月日期" type="number" min={1} max={28} value={day} onChange={(event) => { const selectedDay = event.currentTarget.value; setDay(selectedDay); apply(selection, selectedDay, time, delay); }} /></label><label>时间<input aria-label="月报公开时间" type="time" step={1} value={time} onChange={(event) => { const selectedTime = event.currentTarget.value; setTime(selectedTime); apply(selection, day, selectedTime, delay); }} /></label></>}
    <label>可选随机延迟<select aria-label="月报随机延迟" value={delay} onChange={(event) => { const selectedDelay = event.currentTarget.value; setDelay(selectedDelay); apply(selection, day, time, selectedDelay); }}>
      <option value="0">不延迟</option>{Array.from({ length: 31 }, (_, index) => index + 1).map((days) => <option key={days} value={days}>随机晚0至{days}天</option>)}
    </select></label>
    <p>自定义只允许次月1–28日，避免无效月份日期；不自动截断。延迟按本局 seed、公司与期间一次确定，恢复不重新抽取。</p>
    {value === "Monthly" && <p role="status">请明确选择月报排期后再创建新局。</p>}
    {error !== null && <p role="alert">{error}</p>}
  </div>;
}
