import { useEffect, useState } from "react";
import type { MovingAverageSetting } from "../config/moving-average-settings.ts";
interface Props { readonly settings: readonly MovingAverageSetting[]; readonly save: (settings: readonly MovingAverageSetting[]) => boolean; readonly error: string | null; readonly initiallyOpen?: boolean }
export function MovingAverageSettings({ settings, save, error, initiallyOpen = false }: Props) {
  const [draft, setDraft] = useState(() => settings.map((item) => ({ period: String(item.period), visible: item.visible })));
  const [inputError, setInputError] = useState<string | null>(null);
  useEffect(() => { setDraft(settings.map((item) => ({ period: String(item.period), visible: item.visible }))); setInputError(null); }, [settings]);
  return <details open={initiallyOpen}><summary>均线设置</summary><form aria-label="MA周期与显示设置" onSubmit={(event) => {
    event.preventDefault();
    try {
      const next = draft.map((item) => {
        if (!/^[1-9]\d*$/.test(item.period) || !Number.isSafeInteger(Number(item.period))) throw new Error("周期必须输入正安全整数，不能为小数或空值");
        return { period: Number(item.period), visible: item.visible };
      });
      if (new Set(next.map((item) => item.period)).size !== next.length) throw new Error("MA周期不能重复");
      if (save(next)) setInputError(null);
    } catch (failure) { setInputError(`MA设置无效：${failure instanceof Error ? failure.message : String(failure)}`); }
  }}>
    {draft.map((item, index) => <div key={index}><label>周期<input aria-label={`MA第${index + 1}条周期`} type="number" min="1" step="1" value={item.period} onChange={(event) => setDraft(draft.map((row, position) => position === index ? { ...row, period: event.currentTarget.value } : row))} /></label><label><input aria-label={`MA第${index + 1}条显示`} type="checkbox" checked={item.visible} onChange={(event) => setDraft(draft.map((row, position) => position === index ? { ...row, visible: event.currentTarget.checked } : row))} />显示</label><button type="button" onClick={() => setDraft(draft.filter((_, position) => position !== index))}>删除</button></div>)}
    <button type="button" onClick={() => setDraft([...draft, { period: "", visible: true }])}>新增均线</button><button type="submit">保存均线设置</button><p>默认MA5/10/20/30/60；周期按当前K线根数计算，完整窗口不足时不出值。Desktop与Mobile共用本机显示偏好。</p>
    {inputError !== null && <p role="alert">{inputError}</p>}{error !== null && <p role="alert">{error}</p>}
  </form></details>;
}
