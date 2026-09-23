# Task 9 corpus comparison — incomplete

Status: **INCOMPLETE (9/10 required surfaces)**. This is not a complete Task 9 PASS.

Nine constructible surfaces passed exact comparison. Their result paths and SHA-256 receipts are recorded in `corpus-diff.incomplete.json`. Each comparison also rejected the three mandatory negative mutations: deleted event, fixed-identity payload swap, and unmapped payload mutation.

The missing surface is `equivalence:normal-multi-leg-terminal`. It requires a sealed historical old-engine witness in which the relevant account starts with zero cash and the normal multi-leg terminal path is accepted. In active sealed attempt 12, every `equivalence-1` through `equivalence-10` fixture initializes `AccountId(0)` with `10000000` cents. Consequently, those fixtures cannot prove the required old-side zero-cash acceptance behavior.

Project evidence policy forbids manufacturing a witness, relabeling another scenario, or weakening the gate. The blocker therefore remains a real missing-historical-witness failure until valid historical evidence exists.
