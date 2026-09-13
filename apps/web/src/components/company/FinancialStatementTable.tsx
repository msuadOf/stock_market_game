import type { StatementSection } from "./company-presentation.ts";
import { formatAccountingAmount } from "./company-presentation.ts";

interface FinancialStatementTableProps {
  readonly statement: StatementSection;
  readonly exactAmountsVisible: boolean;
}

export function FinancialStatementTable({ statement, exactAmountsVisible }: FinancialStatementTableProps) {
  return (
    <div className="company-table-wrap" data-testid={`company-statement-${statement.id}`}>
      <table className="company-table" aria-label={statement.title}>
        <thead><tr><th scope="col">科目</th><th scope="col">{exactAmountsVisible ? "金额（元，精确值）" : "金额（缩写）"}</th></tr></thead>
        <tbody>
          {statement.rows.map((row) => (
            <tr key={row.subject}>
              <th scope="row">{row.subject}</th>
              <td title={`精确值：${row.amount}`}>{exactAmountsVisible ? row.amount : formatAccountingAmount(row.amount)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
