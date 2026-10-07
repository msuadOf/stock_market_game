import assert from "node:assert/strict";
import test from "node:test";

import { parseCompanyPreferenceRejections, parseRejectedRightsSubscriptions } from "./corporate-action-views.ts";

const REJECTED_SUBSCRIPTION = {
  event_id: "rights-event", account: "0", requested_shares: "41",
  submitted_on: "2030-01-07", rejected_on: "2030-01-07",
  reason: "公开配售剩余额度 40 股，申请 41 股超出额度",
};

test("配股认购拒绝回执 owner 查询面按 engine wire 形状严格解析", () => {
  assert.deepEqual(parseRejectedRightsSubscriptions([REJECTED_SUBSCRIPTION]), [REJECTED_SUBSCRIPTION]);
  assert.deepEqual(parseRejectedRightsSubscriptions([]), []);
  assert.throws(() => parseRejectedRightsSubscriptions([{ ...REJECTED_SUBSCRIPTION, extra: 1 }]), /必填|extra/);
  assert.throws(() => parseRejectedRightsSubscriptions([{ ...REJECTED_SUBSCRIPTION, event_id: " " }]), /事件身份或原因不能为空/);
  assert.throws(() => parseRejectedRightsSubscriptions([{ ...REJECTED_SUBSCRIPTION, reason: "" }]), /事件身份或原因不能为空/);
  assert.throws(() => parseRejectedRightsSubscriptions([{ ...REJECTED_SUBSCRIPTION, account: "account-0" }]), /规范账户十进制字符串/);
  assert.throws(() => parseRejectedRightsSubscriptions([{ ...REJECTED_SUBSCRIPTION, requested_shares: "0" }]), /必须为正数/);
  assert.throws(() => parseRejectedRightsSubscriptions([{ ...REJECTED_SUBSCRIPTION, rejected_on: "2030-01-06" }]), /拒绝日期不得早于提交日期/);
  assert.throws(() => parseRejectedRightsSubscriptions([{ ...REJECTED_SUBSCRIPTION, submitted_on: "2030-1-7" }]), /submitted_on/);
});

const PREFERENCE_REJECTION = {
  company: "C-600101", evaluated_on: "2030-01-31", kind: "StockDistribution",
  detail: "累计股本扩张已达上限，不再提案",
};

test("偏好拒绝台账 owner 查询面按 engine wire 形状严格解析", () => {
  assert.deepEqual(parseCompanyPreferenceRejections([PREFERENCE_REJECTION]), [PREFERENCE_REJECTION]);
  assert.throws(() => parseCompanyPreferenceRejections([{ ...PREFERENCE_REJECTION, kind: "RightsOffering" }]), /kind/);
  assert.throws(() => parseCompanyPreferenceRejections([{ ...PREFERENCE_REJECTION, company: " " }]), /公司身份不能为空/);
  assert.throws(() => parseCompanyPreferenceRejections([{ ...PREFERENCE_REJECTION, detail: " " }]), /拒绝原因必须非空/);
  assert.throws(() => parseCompanyPreferenceRejections([{ ...PREFERENCE_REJECTION, evaluated_on: "2030-13-01" }]), /evaluated_on/);
  assert.throws(() => parseCompanyPreferenceRejections([null]), /必须是对象/);
});
