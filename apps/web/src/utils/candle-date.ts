export function candleDate(time: number): Date {
  if (!Number.isSafeInteger(time) || time % 86400 !== 0) throw new RangeError("日 K 日期必须是UTC零点的Unix秒标签");
  const date = new Date(time * 1000);
  if (!Number.isFinite(date.getTime()) || date.getUTCFullYear() < 1998 || date.getUTCFullYear() > 2199) throw new RangeError("日 K 日期超出1998至2199年公历可表示范围");
  return date;
}
