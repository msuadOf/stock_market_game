import type { StatementCell, StatementSection } from "./company-presentation.ts";
import { formatAccountingAmount } from "./company-presentation.ts";

interface FinancialStatementTableProps {
  readonly statement: StatementSection;
  readonly exactAmountsVisible: boolean;
}

export function FinancialStatementTable({ statement, exactAmountsVisible }: FinancialStatementTableProps) {
  const renderCell = (cell: StatementCell, index: number) => typeof cell === "string"
    ? <td key={index} title={`精确值：${cell}`}>{exactAmountsVisible ? cell : formatAccountingAmount(cell)}</td>
    : <td key={index} className="unavailable">{cell.text}</td>;
  return (
    <div className="company-table-wrap" data-testid={`company-statement-${statement.id}`}>
      <table className="company-table" aria-label={statement.title}>
        <caption>{exactAmountsVisible ? "金额（元，精确值）" : "金额（缩写，元/万元/亿元）"}</caption>
        <thead><tr><th scope="col">科目</th>{(statement.columns === undefined ? ["金额"] : statement.columns).map((column) => <th key={column} scope="col">{column}</th>)}</tr></thead>
        <tbody>
          {statement.rows.map((row) => (
            <tr key={row.subject}>
              <th scope="row">{row.subject}</th>
              {[row.amount, ...(row.comparisons === undefined ? [] : row.comparisons)].map(renderCell)}
            </tr>
          ))}
        </tbody>
        {statement.details !== undefined && statement.details.length > 0 && <tfoot>{statement.details.map((row) => <tr key={row.subject}><th scope="row">{row.subject}</th><td colSpan={statement.columns === undefined ? 1 : statement.columns.length} title={`精确值：${row.amount}`}>{exactAmountsVisible ? row.amount : formatAccountingAmount(row.amount)}</td></tr>)}</tfoot>}
      </table>
    </div>
  );
}
