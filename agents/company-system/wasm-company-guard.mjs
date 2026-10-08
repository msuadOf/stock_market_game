// DOM 会先清 error，只有本次 UI request 的成功返回和消费者完成共同成立才能验收。
export function companyPanelCompletion({ baseline, generation, workerId, required, trace, ui }) {
  const pending = [];
  const failures = [];
  const matched = [];
  if (generation !== baseline.generation || workerId !== baseline.workerId) {
    failures.push({ message: "公司查询期间 Worker/generation 已切换", baseline, generation, workerId });
  }
  if (required.length === 0) failures.push({ message: "公司阶段缺少必需查询清单" });
  for (const alert of ui.alerts) {
    if (baseline.priorAlerts !== undefined && baseline.priorAlerts.includes(alert)) pending.push(`刷新前的 UI 错误尚未由本次成功消费清除：${alert}`);
    else failures.push({ message: alert, source: "UI" });
  }
  for (const event of trace) {
    if (event.direction === "received" && event.workerId === baseline.workerId && event.sequence > baseline.sequence
      && event.data.generation === baseline.generation && event.data.type === "failure") {
      failures.push({ ...event.data, source: "Worker", sequence: event.sequence });
    }
  }
  for (const requirement of required) {
    const requests = trace.filter((event) => event.direction === "sent" && event.origin === "ui"
      && event.workerId === baseline.workerId && event.sequence > requirement.after
      && event.data.generation === baseline.generation && event.data.type === requirement.type
      && (requirement.companyId === undefined || (event.data.company === requirement.companyId || event.data.query?.company_id === requirement.companyId))
      && (!requirement.rootPage || event.data.query?.cursor === null));
    if (requests.length < requirement.minRequests) pending.push(`${requirement.type}：UI request ${requests.length}/${requirement.minRequests}`);
    for (const request of requests) {
      const replies = trace.filter((event) => event.direction === "received" && event.workerId === request.workerId
        && event.sequence > request.sequence && event.data.generation === request.data.generation
        && event.data.requestId === request.data.requestId);
      const errors = replies.filter((event) => event.data.type === "operationError");
      if (errors.length > 0) {
        for (const error of errors) failures.push({ ...error.data, requestType: request.data.type, sequence: error.sequence });
        continue;
      }
      const unexpected = replies.find((event) => event.data.type !== requirement.successType);
      if (unexpected !== undefined) {
        failures.push({ message: `${requirement.type} 响应类型错误：${unexpected.data.type}`, response: unexpected.data });
        continue;
      }
      const response = replies.find((event) => event.data.type === requirement.successType);
      if (response === undefined) {
        pending.push(`${requirement.type}：generation=${request.data.generation} requestId=${request.data.requestId} 尚未成功返回`);
      } else {
        matched.push({ type: requirement.type, requestId: request.data.requestId, generation: request.data.generation,
          workerId: request.workerId, requestSequence: request.sequence, responseSequence: response.sequence });
      }
    }
    for (const name of requirement.widgets) {
      const widget = ui.widgets[name];
      if (widget === undefined || !widget.visible || widget.loading || !widget.dataVisible) pending.push(`${name}：消费者尚未完成或实际数据不可见`);
    }
  }
  return { status: failures.length > 0 ? "failed" : pending.length > 0 ? "pending" : "passed", pending, failures, matched };
}
