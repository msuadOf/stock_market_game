import { useMemo } from "react";
import type { EngineHost } from "../../host/engine-host.ts";
import { CompanyContractPanel } from "./CompanyContractPanel.tsx";
import { DividendTaxPanel } from "./DividendTaxPanel.tsx";

export interface CompanyReadContext {
  readonly host: EngineHost | null;
  readonly owner: string | null;
  readonly refreshKey: string;
}

/** 仅消费当前宿主的只读查询；本人数据不接受 UI 传入账户参数。 */
export function CompanyReadPanel({ companyCode, companyId, context }: {
  readonly companyCode: string;
  readonly companyId: number;
  readonly context: CompanyReadContext;
}) {
  const { host, owner, refreshKey } = context;
  const queries = useMemo(() => {
    if (host === null || owner === null) return null;
    return {
      capabilities: host.queryCompanyCapabilities === undefined ? undefined : () => host.queryCompanyCapabilities!(companyCode),
      rights: host.queryOwnerRightsOfferings === undefined ? undefined : () => host.queryOwnerRightsOfferings!(),
      receipts: host.queryOwnerFlatWithholdingReceipts === undefined ? undefined : () => host.queryOwnerFlatWithholdingReceipts!(),
      explanation: host.queryCompanyPeriodExplanation === undefined ? undefined : (_company: number, periodEnd: string) => host.queryCompanyPeriodExplanation!(companyCode, periodEnd),
      tax: host.queryAccountDividendTaxStatus === undefined || host.queryDividendTaxOutstandingViews === undefined ? undefined : async () => {
        const [status, outstanding] = await Promise.all([host.queryAccountDividendTaxStatus!(), host.queryDividendTaxOutstandingViews!()]);
        return { status, outstanding };
      },
    };
  }, [host, owner, companyCode]);
  if (host === null) return <p className="company-state">游戏引擎尚未就绪，不能查询公司契约事实。</p>;
  if (owner === null || queries === null) return <p className="company-state">尚未加入市场，不能查询本人公司权益与税务事实。</p>;
  return <>
    <CompanyContractPanel companyId={companyId} refreshKey={refreshKey} onCapabilitiesQuery={queries.capabilities} onOwnerRightsQuery={queries.rights} onFlatReceiptsQuery={queries.receipts} onExplanationQuery={queries.explanation} />
    <DividendTaxPanel refreshKey={refreshKey} onQuery={queries.tax} />
  </>;
}
