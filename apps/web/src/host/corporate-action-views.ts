import { array, civilDate, decimal, exact, oneOf, record, string } from "../save/schema/primitives.ts";

/**
 * 配股认购拒绝回执（owner 查询面）：与 engine `RejectedRightsSubscription`
 * 的 serde 序列化形态一致（同存档 wire 形状）；宿主查询返回后由此严格 parser
 * 校验，不用宽松 any 透传。
 */
export interface RejectedRightsSubscriptionView {
  readonly event_id: string;
  readonly account: string;
  readonly requested_shares: string;
  readonly submitted_on: string;
  readonly rejected_on: string;
  readonly reason: string;
}

export function parseRejectedRightsSubscriptions(value: unknown, path = "rejected_rights_subscriptions"): readonly RejectedRightsSubscriptionView[] {
  return array(value, path).map((item, index) => {
    const rowPath = `${path}[${index}]`;
    const row = record(item, rowPath);
    exact(row, ["event_id", "account", "requested_shares", "submitted_on", "rejected_on", "reason"], rowPath);
    const event_id = string(row.event_id, `${rowPath}.event_id`);
    const account = string(row.account, `${rowPath}.account`);
    const requested_shares = decimal(row.requested_shares, `${rowPath}.requested_shares`);
    const submitted_on = civilDate(row.submitted_on, `${rowPath}.submitted_on`);
    const rejected_on = civilDate(row.rejected_on, `${rowPath}.rejected_on`);
    const reason = string(row.reason, `${rowPath}.reason`);
    if (!event_id.trim() || !reason.trim()) throw new Error(`${rowPath} 拒绝回执事件身份或原因不能为空`);
    if (!/^(0|[1-9]\d*)$/.test(account)) throw new Error(`${rowPath}.account 必须是规范账户十进制字符串`);
    if (BigInt(requested_shares) <= 0n) throw new Error(`${rowPath}.requested_shares 必须为正数`);
    if (rejected_on < submitted_on) throw new Error(`${rowPath}.rejected_on 拒绝日期不得早于提交日期`);
    return { event_id, account, requested_shares, submitted_on, rejected_on, reason };
  });
}

/**
 * 排队认购回执（owner 查询面）：engine `QueuedRightsSubscription` 的 serde 形态
 * ——`subscribe_rights_offering` 受理成功的返回值，当日日终划扣；由宿主查询
 * 返回后经此严格 parser 校验，不用宽松 any 透传。owner 隔离由 parser 复核
 * account 必须为 "0"（本机玩家）。
 */
export interface QueuedRightsSubscriptionView {
  readonly event_id: string;
  readonly account: string;
  readonly requested_shares: string;
  readonly submitted_on: string;
}

/** 排队认购回执严格 parser（认购命令的受理回执面）。 */
export function parseQueuedRightsSubscription(value: unknown, path = "queued_rights_subscription"): QueuedRightsSubscriptionView {
  const row = record(value, path);
  exact(row, ["event_id", "account", "requested_shares", "submitted_on"], path);
  const event_id = string(row.event_id, `${path}.event_id`);
  const account = string(row.account, `${path}.account`);
  const requested_shares = decimal(row.requested_shares, `${path}.requested_shares`);
  const submitted_on = civilDate(row.submitted_on, `${path}.submitted_on`);
  if (!event_id.trim()) throw new Error(`${path}.event_id 认购事件身份不能为空`);
  if (!/^(0|[1-9]\d*)$/.test(account)) throw new Error(`${path}.account 必须是规范账户十进制字符串`);
  if (account !== "0") throw new Error(`${path}.account 受理回执必须归属本机玩家（owner 隔离被破坏）`);
  if (BigInt(requested_shares) <= 0n) throw new Error(`${path}.requested_shares 认购股数必须为正`);
  return { event_id, account, requested_shares, submitted_on };
}

/**
 * 公司行为偏好提案拒绝台账（ADR-0037；owner 查询面）：与 engine
 * `SimplePreferenceRejection` 的 serde 形态一致。跨域勾稽（公司身份已知、
 * 评估日不晚于已推进日）在存档提交与恢复时由 engine `validate` 保证。
 */
export interface CompanyPreferenceRejectionView {
  readonly company: string;
  readonly evaluated_on: string;
  readonly kind: "CashDividend" | "StockDistribution";
  readonly detail: string;
}

export function parseCompanyPreferenceRejections(value: unknown, path = "company_preference_rejections"): readonly CompanyPreferenceRejectionView[] {
  return array(value, path).map((item, index) => {
    const rowPath = `${path}[${index}]`;
    const row = record(item, rowPath);
    exact(row, ["company", "evaluated_on", "kind", "detail"], rowPath);
    const company = string(row.company, `${rowPath}.company`);
    const detail = string(row.detail, `${rowPath}.detail`);
    if (!company.trim()) throw new Error(`${rowPath}.company 公司身份不能为空`);
    if (!detail.trim()) throw new Error(`${rowPath}.detail 拒绝原因必须非空`);
    return {
      company,
      evaluated_on: civilDate(row.evaluated_on, `${rowPath}.evaluated_on`),
      kind: oneOf(row.kind, `${rowPath}.kind`, ["CashDividend", "StockDistribution"] as const),
      detail,
    };
  });
}
