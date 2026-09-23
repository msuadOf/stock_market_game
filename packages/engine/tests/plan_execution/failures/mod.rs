mod adoption;
mod auctions;
mod cancellation;
mod remainder;

use super::*;
use engine::session::PlanExecutionDisposition;
use engine::OrderId;

fn submitted_id(report: &engine::session::PlanExecutionReport) -> OrderId {
    match report.disposition {
        PlanExecutionDisposition::Submitted { order_id, .. } => order_id,
        ref other => panic!("expected submission, got {other:?}"),
    }
}
