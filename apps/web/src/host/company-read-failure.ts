import type { CompanyErrorClass } from "../types/generated/CompanyErrorClass.ts";
import type { ValidationFault } from "../types/generated/ValidationFault.ts";
import { boundedInteger, ownerAccount, nonempty } from "./company-contract-views.ts";
import { exact, oneOf, record } from "../save/schema/primitives.ts";

export type CompanyReadFailure =
  | { readonly kind: "Unhealthy" }
  | { readonly kind: "UnknownOwner"; readonly data: { readonly account: string } }
  | { readonly kind: "UnknownCompany"; readonly data: { readonly company: number } }
  | { readonly kind: "Catalog"; readonly data: { readonly fault: ValidationFault } }
  | { readonly kind: "Company"; readonly data: { readonly classification: CompanyErrorClass; readonly reason: string } }
  | { readonly kind: "CorporateState" | "DividendTaxOutstanding"; readonly data: { readonly reason: string } };

/** 严格解析 company-read typed failure；不从 message、Display 或缺省 data 推断原因。 */
export function parseCompanyReadFailure(value: unknown): CompanyReadFailure {
  const path = "CompanyReadFailure";
  const wrapper = record(value, path);
  if (typeof wrapper.kind !== "string") throw new Error(`${path}.kind 必须是字符串`);
  const kind = oneOf(wrapper.kind, `${path}.kind`, ["Unhealthy", "UnknownOwner", "UnknownCompany", "Catalog", "Company", "CorporateState", "DividendTaxOutstanding"] as const);
  if (kind === "Unhealthy") {
    exact(wrapper, ["kind"], path);
    return { kind };
  }
  exact(wrapper, ["kind", "data"], path);
  const data = record(wrapper.data, `${path}.data`);
  if (kind === "UnknownOwner") {
    exact(data, ["account"], `${path}.data`);
    return { kind, data: { account: ownerAccount(data.account, `${path}.data.account`) } };
  }
  if (kind === "UnknownCompany") {
    exact(data, ["company"], `${path}.data`);
    return { kind, data: { company: boundedInteger(data.company, `${path}.data.company`, 0, 4294967295) } };
  }
  if (kind === "Catalog") {
    exact(data, ["fault"], `${path}.data`);
    const raw = record(data.fault, `${path}.data.fault`);
    const hasContext = Object.hasOwn(raw, "context");
    exact(raw, hasContext ? ["code", "context"] : ["code"], `${path}.data.fault`);
    const code = boundedInteger(raw.code, `${path}.data.fault.code`, 0, 65535);
    const context = !hasContext ? undefined : (() => {
      if (!Array.isArray(raw.context)) throw new Error(`${path}.data.fault.context 必须是数组`);
      return raw.context.map((value, index) => boundedInteger(value, `${path}.data.fault.context[${index}]`, 0, Number.MAX_SAFE_INTEGER));
    })();
    return { kind, data: { fault: context === undefined ? { code } : { code, context } } };
  }
  if (kind === "Company") {
    exact(data, ["classification", "reason"], `${path}.data`);
    return { kind, data: {
      classification: oneOf(data.classification, `${path}.data.classification`, ["InvalidInput", "BusinessCondition", "UnsupportedOperation", "SystemState"] as const),
      reason: nonempty(data.reason, `${path}.data.reason`),
    } };
  }
  exact(data, ["reason"], `${path}.data`);
  return { kind, data: { reason: nonempty(data.reason, `${path}.data.reason`) } };
}

export class CompanyReadRejectedError extends Error {
  readonly failure: CompanyReadFailure;

  constructor(failure: CompanyReadFailure, cause: unknown) {
    const message = failure.kind === "Company" || failure.kind === "CorporateState" || failure.kind === "DividendTaxOutstanding"
      ? failure.data.reason
      : `公司只读查询失败：${failure.kind}`;
    super(message, { cause });
    this.name = "CompanyReadRejectedError";
    this.failure = failure;
  }
}
