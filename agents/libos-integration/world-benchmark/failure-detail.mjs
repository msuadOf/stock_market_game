export function failureDetail(error, seen = new Set()) {
  if (!(error instanceof Error)) return {message:String(error)};
  if (seen.has(error)) return {message:'错误引用循环'};
  seen.add(error);
  const detail = {name:error.name, message:error.message, stack:error.stack};
  if (error.cause !== undefined) detail.cause = failureDetail(error.cause, seen);
  if (error instanceof AggregateError) detail.errors = Array.from(error.errors, item => failureDetail(item, seen));
  return detail;
}
