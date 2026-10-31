// 当前 Rust 公司状态 wire 字段表；来源与逐文件 SHA 保存在 source-b-web-strict-save/rust-field-table.json。
// 仅描述结构与 codec，不复制会计、权益、审批或日结业务算法。
// repurchase_execution_bindings 增量依据冻结源码 6d45be0a；静态证据见 source-b-web-strict-save/repurchase-binding-web/static-contract.json。
export type StateRule = string | readonly [string, ...unknown[]];
export const STATE_SHAPES: Readonly<Record<string, StateRule>> = {
  "CompanySystem": [
    "object",
    {
      "issuers": "issuerMap",
      "implementation": [
        "ref",
        "CompanyImplementation"
      ]
    }
  ],
  "SimpleFundamentals": [
    "object",
    {
      "config": "simpleConfig",
      "history_start": "date",
      "advanced_through": "date",
      "environment_change_bp": "i32",
      "environment_rng": "rng",
      "companies": [
        "map",
        "u32",
        [
          "ref",
          "SimpleCompanyState"
        ]
      ],
      "history": [
        "array",
        [
          "ref",
          "SimpleDisclosureCandidate"
        ]
      ]
    }
  ],
  "SessionCorporateActions": [
    "object",
    {
      "registries": [
        "array",
        [
          "ref",
          "ShareRegistry"
        ]
      ],
      "dividends": [
        "array",
        [
          "ref",
          "CashDividendBook"
        ]
      ],
      "dividend_tax_books": [
        "array",
        [
          "ref",
          "CashDividendTaxBook"
        ]
      ],
      "flat_withholding_receipts": [
        "array",
        [
          "ref",
          "FlatWithholdingReceipt"
        ]
      ],
      "stock_distributions": [
        "array",
        [
          "ref",
          "StockDistributionBook"
        ]
      ],
      "share_splits": [
        "array",
        [
          "ref",
          "ShareSplitBook"
        ]
      ],
      "rights_offerings": [
        "array",
        [
          "ref",
          "RightsOfferingBook"
        ]
      ],
      "rights_subscription_queue": [
        "array",
        [
          "ref",
          "QueuedRightsSubscription"
        ]
      ],
      "rejected_rights_subscriptions": [
        "array",
        [
          "ref",
          "RejectedRightsSubscription"
        ]
      ],
      "issuer_repurchases": [
        "array",
        [
          "ref",
          "IssuerRepurchaseBook"
        ]
      ],
      "repurchase_execution_bindings": [
        "array",
        [
          "ref",
          "RepurchaseExecutionBinding"
        ]
      ],
      "account_gross_receipts": [
        "array",
        [
          "ref",
          "AccountDividendGrossReceipt"
        ]
      ],
      "external_receipts": [
        "array",
        [
          "ref",
          "ExternalDividendReceipt"
        ]
      ],
      "applied_ex_reference_groups": [
        "array",
        [
          "ref",
          "AppliedExReferenceGroup"
        ]
      ]
    }
  ],
  "RepurchaseExecutionBinding": [
    "object",
    {
      "event_id": "string",
      "issuer": "u32",
      "stock": "string",
      "account": "account",
      "plan_id": [
        "ref",
        "PlanId"
      ],
      "funding_committed": "boolean",
      "funding_amount": "money",
      "applied_fact_cursor": "canonicalU64",
      "closing_state": [
        "ref",
        "RepurchaseBindingClosingState"
      ]
    }
  ],
  "PlanId": [
    "object",
    {
      "owner": "account",
      "sequence": "canonicalU64"
    }
  ],
  "RepurchaseBindingClosingState": [
    "enum",
    {
      "Active": null,
      "Draining": null,
      "Closed": null
    }
  ],
  "CompanyImplementation": [
    "object",
    {
      "mode": [
        "literal",
        "Simple"
      ],
      "state": [
        "ref",
        "SimpleFundamentals"
      ]
    }
  ],
  "SimpleCompanyState": [
    "object",
    {
      "generation": [
        "ref",
        "PeriodGenerationState"
      ],
      "finance": [
        "ref",
        "SimpleFinanceState"
      ],
      "pending_restart": [
        "nullable",
        [
          "tuple",
          [
            "amount",
            "string"
          ]
        ]
      ],
      "preference_ledger": [
        "ref",
        "SimplePreferenceLedger"
      ]
    }
  ],
  "SimpleDisclosureCandidate": [
    "object",
    {
      "company": "u32",
      "period_start": "date",
      "period_end": "date",
      "amounts": [
        "ref",
        "PeriodAmounts"
      ],
      "explanation": [
        "ref",
        "PeriodChangeExplanation"
      ]
    }
  ],
  "ShareRegistry": [
    "object",
    {
      "stock": "string",
      "issuer": "u32",
      "issued_shares": "canonicalU64",
      "issuer_repurchase_account": [
        "nullable",
        [
          "ref",
          "IssuerRepurchaseAccountFacts"
        ]
      ],
      "settled_on": "date",
      "holdings": [
        "array",
        [
          "ref",
          "ShareHolding"
        ]
      ],
      "receipts": [
        "array",
        [
          "ref",
          "ShareDayReceipt"
        ]
      ],
      "registrations": [
        "array",
        [
          "ref",
          "RegistrationSnapshot"
        ]
      ],
      "pending_opening": ["nullable", ["ref", "PendingOpening"]]
    }
  ],
  "PendingOpening": ["object", {
    "predecessor": ["ref", "ShareDayFrontier"],
    "opening": ["ref", "FrozenOpeningMovement"]
  }],
  "FrozenOpeningMovement": ["object", {
    "reference": ["ref", "OpeningMovementRef"],
    "request": ["ref", "ShareDayRequest"]
  }],
  "CashDividendBook": [
    "object",
    {
      "plan": [
        "ref",
        "CashDividendPlan"
      ],
      "status": [
        "ref",
        "CashDividendStatus"
      ],
      "registration": [
        "nullable",
        [
          "ref",
          "RegistrationSnapshot"
        ]
      ],
      "entitlements": [
        "array",
        [
          "ref",
          "CashDividendEntitlement"
        ]
      ],
      "paid": [
        "array",
        [
          "tuple",
          [
            [
              "ref",
              "HolderId"
            ],
            "money"
          ]
        ]
      ],
      "failures": [
        "array",
        [
          "tuple",
          [
            [
              "ref",
              "HolderId"
            ],
            "string"
          ]
        ]
      ],
      "payments": [
        "array",
        [
          "ref",
          "CashDividendPaymentReceipt"
        ]
      ]
    }
  ],
  "CashDividendTaxBook": [
    "object",
    {
      "operation_seq": "u64number",
      "account": "account",
      "stock": "string",
      "profile": [
        "ref",
        "DividendTaxProfile"
      ],
      "opened_on": "date",
      "opening_lots": [
        "array",
        [
          "ref",
          "DividendTaxLot"
        ]
      ],
      "settled_on": "date",
      "lots": [
        "array",
        [
          "ref",
          "DividendTaxLot"
        ]
      ],
      "days": [
        "array",
        [
          "ref",
          "TaxDayReceipt"
        ]
      ],
      "dividends": [
        "array",
        [
          "ref",
          "RegisteredTaxDividend"
        ]
      ],
      "collections": [
        "array",
        [
          "ref",
          "TaxCollectionReceipt"
        ]
      ],
      "redenominations": [
        "array",
        [
          "ref",
          "TaxRedenominationReceipt"
        ]
      ],
      "stock_bonus_bases": [
        "array",
        [
          "ref",
          "StockBonusTaxBasis"
        ]
      ]
    }
  ],
  "FlatWithholdingReceipt": [
    "object",
    {
      "payment_id": "string",
      "plan_id": "string",
      "account": "account",
      "paid_on": "date",
      "gross": "money",
      "rate_bp": "u64number",
      "withheld": "money"
    }
  ],
  "StockDistributionBook": [
    "object",
    {
      "plan": [
        "ref",
        "StockDistributionEventPlan"
      ],
      "status": [
        "ref",
        "StockDistributionStatus"
      ],
      "registration": [
        "nullable",
        [
          "ref",
          "RegistrationSnapshot"
        ]
      ],
      "receipt": [
        "nullable",
        [
          "ref",
          "StockDistributionReceipt"
        ]
      ],
      "credited_on": [
        "nullable",
        "date"
      ]
    }
  ],
  "ShareSplitBook": [
    "object",
    {
      "plan": [
        "ref",
        "ShareSplitEventPlan"
      ],
      "status": [
        "ref",
        "ShareSplitStatus"
      ],
      "registration": [
        "nullable",
        [
          "ref",
          "RegistrationSnapshot"
        ]
      ],
      "receipt": [
        "nullable",
        [
          "ref",
          "ShareSplitReceipt"
        ]
      ],
      "settled_on": [
        "nullable",
        "date"
      ]
    }
  ],
  "RightsOfferingBook": [
    "object",
    {
      "plan": [
        "ref",
        "RightsOfferingEventPlan"
      ],
      "status": [
        "ref",
        "RightsOfferingStatus"
      ],
      "registration": [
        "nullable",
        [
          "ref",
          "RegistrationSnapshot"
        ]
      ],
      "entitlement": [
        "nullable",
        [
          "ref",
          "RightsEntitlementReceipt"
        ]
      ],
      "subscriptions": [
        "array",
        [
          "ref",
          "RightsSubscriptionRecord"
        ]
      ],
      "closed_on": [
        "nullable",
        "date"
      ],
      "settlement": [
        "nullable",
        [
          "ref",
          "RightsSettlementReceipt"
        ]
      ],
      "credited_on": [
        "nullable",
        "date"
      ]
    }
  ],
  "QueuedRightsSubscription": [
    "object",
    {
      "event_id": "string",
      "requested_shares": "canonicalU64",
      "submitted_on": "date",
      "account": "account"
    }
  ],
  "RejectedRightsSubscription": [
    "object",
    {
      "event_id": "string",
      "account": "account",
      "requested_shares": "canonicalU64",
      "submitted_on": "date",
      "rejected_on": "date",
      "reason": "string"
    }
  ],
  "IssuerRepurchaseBook": [
    "object",
    {
      "plan": [
        "ref",
        "IssuerRepurchasePlan"
      ],
      "status": [
        "ref",
        "IssuerRepurchaseStatus"
      ],
      "fills": [
        "array",
        [
          "ref",
          "RepurchaseFillRecord"
        ]
      ],
      "share_unit_adjustments": [
        "array",
        [
          "ref",
          "ShareUnitAdjustment"
        ]
      ],
      "completed_on": [
        "nullable",
        "date"
      ],
      "withdrawn_remainder": [
        "nullable",
        "money"
      ],
      "cancelled_on": [
        "nullable",
        "date"
      ],
      "cancelled_shares": "canonicalU64",
      "last_order_day": [
        "nullable",
        "date"
      ]
    }
  ],
  "AccountDividendGrossReceipt": [
    "object",
    {
      "payment_id": "string",
      "plan_id": "string",
      "account": "account",
      "paid_on": "date",
      "gross": "money",
      "tax_status": [
        "ref",
        "DividendTaxStatus"
      ]
    }
  ],
  "ExternalDividendReceipt": [
    "object",
    {
      "payment_id": "string",
      "plan_id": "string",
      "holder": [
        "ref",
        "HolderId"
      ],
      "paid_on": "date",
      "gross": "money",
      "tax_status": [
        "ref",
        "DividendTaxStatus"
      ]
    }
  ],
  "AppliedExReferenceGroup": [
    "object",
    {
      "date": "date",
      "stock": "string",
      "cash_plan_ids": [
        "array",
        "string"
      ],
      "stock_event_ids": [
        "array",
        "string"
      ],
      "rights_event_ids": [
        "array",
        "string"
      ],
      "split_event_ids": [
        "array",
        "string"
      ],
      "pre_ex_close": "money",
      "reference": [
        "ref",
        "ExReferencePrice"
      ]
    }
  ],
  "PeriodGenerationState": [
    "object",
    {
      "amounts": [
        "ref",
        "PeriodAmounts"
      ],
      "rng": "rng",
      "revenue_trend": [
        "ref",
        "AnnualTrendState"
      ],
      "fixed_expense_trend": [
        "ref",
        "AnnualTrendState"
      ],
      "variable_expense_trend": [
        "nullable",
        [
          "ref",
          "AnnualTrendState"
        ]
      ]
    }
  ],
  "SimpleFinanceState": [
    "object",
    {
      "company": "u32",
      "kind": "companyKind",
      "config": "simpleFinanceConfig",
      "books": "books",
      "closing": "closing",
      "opening_date": "date",
      "as_of": "date",
      "last_month": "period",
      "next_event_id": "canonicalU64",
      "income_tax_position": "incomeTax",
      "recognized_periods": [
        "array",
        [
          "tuple",
          [
            "date",
            "date"
          ]
        ]
      ],
      "dividends": [
        "map",
        "string",
        [
          "ref",
          "DividendPlanState"
        ]
      ],
      "stock_distributions": [
        "map",
        "string",
        [
          "ref",
          "StockDistributionFinanceFact"
        ]
      ],
      "share_splits": [
        "map",
        "string",
        [
          "ref",
          "ShareSplitFinanceFact"
        ]
      ],
      "rights_offerings": [
        "map",
        "string",
        [
          "ref",
          "RightsOfferingFinanceFact"
        ]
      ],
      "issuer_repurchases": [
        "map",
        "string",
        [
          "ref",
          "IssuerRepurchaseFinanceFact"
        ]
      ],
      "legal_facts": [
        "nullable",
        [
          "ref",
          "DividendLegalFacts"
        ]
      ],
      "cash_book": "amount",
      "investment_book": "amount"
    }
  ],
  "SimplePreferenceLedger": [
    "object",
    {
      "rejections": [
        "array",
        [
          "ref",
          "SimplePreferenceRejection"
        ]
      ]
    }
  ],
  "PeriodAmounts": [
    "object",
    {
      "revenue": "amount",
      "fixed_expense": "amount",
      "variable_expense": "amount"
    }
  ],
  "PeriodChangeExplanation": [
    "object",
    {
      "previous": [
        "ref",
        "PeriodAmounts"
      ],
      "cycle": [
        "ref",
        "SettlementCycle"
      ],
      "environment_change_bp": "i32",
      "demand_contribution_bp": "i32",
      "revenue_segments": [
        "array",
        [
          "ref",
          "TrendSegment"
        ]
      ],
      "fixed_expense_segments": [
        "array",
        [
          "ref",
          "TrendSegment"
        ]
      ],
      "variable_expense_segments": [
        "array",
        [
          "ref",
          "TrendSegment"
        ]
      ],
      "revenue_noise_bp": "i32",
      "fixed_expense_noise_bp": "i32",
      "variable_expense_noise_bp": "i32",
      "restart_revenue": [
        "nullable",
        "amount"
      ],
      "restart_source": [
        "nullable",
        "string"
      ]
    }
  ],
  "IssuerRepurchaseAccountFacts": [
    "object",
    {
      "account_reference": "string",
      "source_evidence": "string",
      "established_on": "date"
    }
  ],
  "ShareHolding": [
    "object",
    {
      "holder": [
        "ref",
        "HolderId"
      ],
      "lots": [
        "array",
        [
          "ref",
          "ShareLot"
        ]
      ]
    }
  ],
  "ShareDayReceipt": [
    "object",
    {
      "request": [
        "ref",
        "ShareDayRequest"
      ],
      "disposals": [
        "array",
        [
          "ref",
          "DisposedLot"
        ]
      ]
    }
  ],
  "RegistrationSnapshot": [
    "object",
    {
      "event_id": "string",
      "stock": "string",
      "issuer": "u32",
      "registered_on": "date",
      "issued_shares": "canonicalU64",
      "settled_receipts": "canonicalU64",
      "issuer_repurchase_account": [
        "nullable",
        [
          "ref",
          "IssuerRepurchaseAccountFacts"
        ]
      ],
      "holdings": [
        "array",
        [
          "ref",
          "ShareHolding"
        ]
      ]
    }
  ],
  "CashDividendPlan": [
    "object",
    {
      "plan_id": "string",
      "issuer": "u32",
      "stock": "string",
      "exchange": [
        "ref",
        "CalendarExchange"
      ],
      "formula": [
        "ref",
        "CashDividendFormula"
      ],
      "approved_on": "date",
      "announced_on": "date",
      "registered_on": "date",
      "ex_dividend_on": "date",
      "payable_on": "date",
      "gross_per_share": "money",
      "distributable_amount": "money"
    }
  ],
  "CashDividendStatus": [
    "enum",
    {
      "Approved": null,
      "Announced": null,
      "Registered": null,
      "Payable": null,
      "PartiallyPaid": null,
      "Paid": null
    }
  ],
  "CashDividendEntitlement": [
    "object",
    {
      "holder": [
        "ref",
        "HolderId"
      ],
      "shares": "canonicalU64",
      "gross": "money"
    }
  ],
  "HolderId": [
    "enum",
    {
      "Account": "account",
      "External": "string",
      "IssuerTreasury": null
    }
  ],
  "CashDividendPaymentReceipt": [
    "object",
    {
      "payment_id": "string",
      "paid_on": "date",
      "within_six_month_deadline": "boolean",
      "outcomes": [
        "array",
        [
          "ref",
          "HolderPaymentOutcome"
        ]
      ]
    }
  ],
  "DividendTaxProfile": [
    "enum",
    {
      "IndividualPublicMarket": null,
      "ResidentEnterprise": null,
      "SecuritiesFund": null,
      "NonResident": null
    }
  ],
  "DividendTaxLot": [
    "object",
    {
      "id": "string",
      "qty": "canonicalU64",
      "acquired_on": "date",
      "source": [
        "ref",
        "TaxAcquisitionSource"
      ],
      "class": [
        "ref",
        "TaxShareClass"
      ]
    }
  ],
  "TaxDayReceipt": [
    "object",
    {
      "operation_seq": "u64number",
      "event_id": "string",
      "day": "date",
      "net_change": "canonicalI128",
      "acquisition": [
        "nullable",
        [
          "ref",
          "DividendTaxLot"
        ]
      ],
      "dispositions": [
        "array",
        [
          "ref",
          "TaxDisposition"
        ]
      ]
    }
  ],
  "RegisteredTaxDividend": [
    "object",
    {
      "operation_seq": "u64number",
      "event_id": "string",
      "registered_on": "date",
      "per_share": [
        "ref",
        "ExactDividendTaxAmount"
      ],
      "lots": [
        "array",
        [
          "ref",
          "DividendTaxLot"
        ]
      ],
      "payments": [
        "array",
        [
          "ref",
          "DividendCashReceipt"
        ]
      ]
    }
  ],
  "TaxCollectionReceipt": [
    "object",
    {
      "operation_seq": "u64number",
      "event_id": "string",
      "day": "date",
      "available_cash": "money",
      "collected": "money",
      "remaining_cash": "money",
      "outstanding": [
        "ref",
        "ExactDividendTaxAmount"
      ],
      "needs_funds": "boolean"
    }
  ],
  "TaxRedenominationReceipt": [
    "object",
    {
      "operation_seq": "u64number",
      "event_id": "string",
      "day": "date",
      "net_change": "canonicalI128",
      "removed": [
        "array",
        [
          "ref",
          "TaxDisposition"
        ]
      ]
    }
  ],
  "StockBonusTaxBasis": [
    "object",
    {
      "operation_seq": "u64number",
      "event_id": "string",
      "credited_on": "date",
      "lot_id": "string",
      "qty": "canonicalU64",
      "par_value_per_share": "money"
    }
  ],
  "StockDistributionEventPlan": [
    "object",
    {
      "event_id": "string",
      "approval_reference": "string",
      "issuer": "u32",
      "stock": "string",
      "exchange": [
        "ref",
        "CalendarExchange"
      ],
      "kind": [
        "ref",
        "StockDistributionKind"
      ],
      "approved_on": "date",
      "announced_on": "date",
      "registered_on": "date",
      "ex_rights_on": "date",
      "shares_per_existing_share_micros": "canonicalU64",
      "approved_total_new_shares": "canonicalU64"
    }
  ],
  "StockDistributionStatus": [
    "enum",
    {
      "Approved": null,
      "Announced": null,
      "Registered": null,
      "Credited": null
    }
  ],
  "StockDistributionReceipt": [
    "object",
    {
      "event_id": "string",
      "approval_reference": "string",
      "registration_event_id": "string",
      "stock": "string",
      "issuer": "u32",
      "registered_on": "date",
      "issued_shares_before": "canonicalU64",
      "issuer_treasury_shares_excluded": "canonicalU64",
      "kind": [
        "ref",
        "StockDistributionKind"
      ],
      "shares_per_existing_share_micros": "canonicalU64",
      "approved_total_new_shares": "canonicalU64",
      "tie_break_seed": "canonicalU64",
      "source_lot_attribution": [
        "ref",
        "SourceLotAttribution"
      ],
      "holders": [
        "array",
        [
          "ref",
          "HolderDistribution"
        ]
      ]
    }
  ],
  "ShareSplitEventPlan": [
    "object",
    {
      "event_id": "string",
      "approval_reference": "string",
      "issuer": "u32",
      "stock": "string",
      "exchange": [
        "ref",
        "CalendarExchange"
      ],
      "direction": [
        "ref",
        "ShareSplitDirection"
      ],
      "ratio": "canonicalU64",
      "approved_on": "date",
      "announced_on": "date",
      "registered_on": "date",
      "ex_rights_on": "date"
    }
  ],
  "ShareSplitStatus": [
    "enum",
    {
      "Approved": null,
      "Announced": null,
      "Registered": null,
      "Settled": null
    }
  ],
  "ShareSplitReceipt": [
    "object",
    {
      "event_id": "string",
      "approval_reference": "string",
      "registration_event_id": "string",
      "stock": "string",
      "issuer": "u32",
      "registered_on": "date",
      "direction": [
        "ref",
        "ShareSplitDirection"
      ],
      "ratio": "canonicalU64",
      "issued_shares_before": "canonicalU64",
      "issued_shares_after": "canonicalU64",
      "tie_break_seed": "canonicalU64",
      "holders": [
        "array",
        [
          "ref",
          "HolderSplitOutcome"
        ]
      ]
    }
  ],
  "RightsOfferingEventPlan": [
    "object",
    {
      "event_id": "string",
      "approval_reference": "string",
      "issuer": "u32",
      "stock": "string",
      "exchange": [
        "ref",
        "CalendarExchange"
      ],
      "approved_on": "date",
      "announced_on": "date",
      "registered_on": "date",
      "payment_start_on": "date",
      "payment_deadline_on": "date",
      "ex_rights_on": "date",
      "settlement_on": "date",
      "listing_on": "date",
      "price_per_share": "money",
      "mode": [
        "ref",
        "RightsOfferingMode"
      ],
      "npc_subscription_strategy": [
        "ref",
        "RightsSubscriptionStrategy"
      ]
    }
  ],
  "RightsOfferingStatus": [
    "enum",
    {
      "Approved": null,
      "Announced": null,
      "Entitled": null,
      "Closed": null,
      "Settled": null
    }
  ],
  "RightsEntitlementReceipt": [
    "object",
    {
      "event_id": "string",
      "approval_reference": "string",
      "registration_event_id": "string",
      "stock": "string",
      "issuer": "u32",
      "registered_on": "date",
      "issued_shares_before": "canonicalU64",
      "issuer_treasury_shares_excluded": "canonicalU64",
      "price_per_share": "money",
      "mode": [
        "ref",
        "RightsOfferingMode"
      ],
      "planned_total_rights_shares": "canonicalU64",
      "tie_break_seed": "canonicalU64",
      "entitlements": [
        "array",
        [
          "ref",
          "RightsEntitlement"
        ]
      ],
      "open_subscription_shares": "canonicalU64"
    }
  ],
  "RightsSubscriptionRecord": [
    "object",
    {
      "holder": [
        "ref",
        "HolderId"
      ],
      "requested_shares": "canonicalU64",
      "price_per_share": "money",
      "submitted_on": "date",
      "origin": [
        "ref",
        "SubscriptionOrigin"
      ],
      "paid_shares": "canonicalU64",
      "paid_amount": "money",
      "waived_shares": "canonicalU64"
    }
  ],
  "RightsSettlementReceipt": [
    "object",
    {
      "event_id": "string",
      "settlement_on": "date",
      "failed": "boolean",
      "total_paid_shares": "canonicalU64",
      "total_paid_amount": "money",
      "refunded_total": "money",
      "holders": [
        "array",
        [
          "ref",
          "HolderRightsSettlement"
        ]
      ]
    }
  ],
  "IssuerRepurchasePlan": [
    "object",
    {
      "event_id": "string",
      "approval_reference": "string",
      "issuer": "u32",
      "stock": "string",
      "exchange": [
        "ref",
        "CalendarExchange"
      ],
      "approved_on": "date",
      "announced_on": "date",
      "window_start_on": "date",
      "window_deadline_on": "date",
      "price_cap_per_share": "money",
      "total_budget": "money",
      "max_shares": "canonicalU64",
      "purpose": [
        "ref",
        "RepurchasePurpose"
      ],
      "completion_policy": [
        "ref",
        "RepurchaseCompletionPolicy"
      ],
      "approval_unit_event_id": [
        "nullable",
        "string"
      ]
    }
  ],
  "IssuerRepurchaseStatus": [
    "enum",
    {
      "Approved": null,
      "Announced": null,
      "Executing": null,
      "Completed": null,
      "Cancelled": null
    }
  ],
  "RepurchaseFillRecord": [
    "object",
    {
      "stock": "string",
      "day": "date",
      "shares": "canonicalU64",
      "gross": "money",
      "fees": "money"
    }
  ],
  "ShareUnitAdjustment": [
    "object",
    {
      "split_event_id": "string",
      "shares_before": "canonicalU64",
      "shares_after": "canonicalU64"
    }
  ],
  "DividendTaxStatus": [
    "enum",
    {
      "FlatWithholding": null,
      "IndividualPublicMarket": null,
      "TreatmentNotConfigured": null
    }
  ],
  "ExReferencePrice": [
    "object",
    {
      "ex_date": "date",
      "reference_price": "money"
    }
  ],
  "AnnualTrendState": [
    "object",
    {
      "annual_growth_bp": "i32",
      "remaining_months": "u16"
    }
  ],
  "DividendPlanState": [
    "object",
    {
      "declaration": [
        "ref",
        "DividendDeclaration"
      ],
      "declaration_source": "u64number",
      "reserve": "amount",
      "reserve_basis_year": [
        "nullable",
        "i32"
      ],
      "payments": [
        "map",
        "string",
        [
          "ref",
          "DividendPaymentState"
        ]
      ]
    }
  ],
  "StockDistributionFinanceFact": [
    "object",
    {
      "event_id": "string",
      "approval_reference": "string",
      "kind": [
        "ref",
        "StockDistributionKind"
      ],
      "approved_on": "date",
      "new_shares": "canonicalU64",
      "par_value_per_share": "money",
      "capital_increase": "amount",
      "registered_capital_at_approval": "amount",
      "credited_on": [
        "nullable",
        "date"
      ],
      "statutory_reserve": "amount",
      "reserve_basis_year": [
        "nullable",
        "i32"
      ]
    }
  ],
  "ShareSplitFinanceFact": [
    "object",
    {
      "event_id": "string",
      "approval_reference": "string",
      "direction": [
        "ref",
        "ShareSplitDirection"
      ],
      "approved_on": "date",
      "ratio": "canonicalU64",
      "par_value_before": "money",
      "par_value_after": "money",
      "registered_capital_at_approval": "amount",
      "issued_shares_before": "canonicalU64",
      "issued_shares_after": "canonicalU64",
      "destroyed_shares": "canonicalU64",
      "registered_capital_reduction": "amount",
      "settled_on": [
        "nullable",
        "date"
      ]
    }
  ],
  "RightsOfferingFinanceFact": [
    "object",
    {
      "event_id": "string",
      "approval_reference": "string",
      "approved_on": "date",
      "price_per_share": "money",
      "par_value_per_share": "money",
      "registered_capital_at_approval": "amount",
      "settled_on": [
        "nullable",
        "date"
      ],
      "issued_shares": "canonicalU64",
      "proceeds": [
        "nullable",
        "amount"
      ],
      "capital_increase": [
        "nullable",
        "amount"
      ]
    }
  ],
  "IssuerRepurchaseFinanceFact": [
    "object",
    {
      "event_id": "string",
      "approval_reference": "string",
      "approved_on": "date",
      "synthetic_funding": "amount",
      "purpose": [
        "ref",
        "RepurchasePurpose"
      ],
      "spent": [
        "nullable",
        "amount"
      ],
      "withdrawn_remainder": [
        "nullable",
        "amount"
      ],
      "completed_on": [
        "nullable",
        "date"
      ],
      "cancelled_shares": "canonicalU64",
      "cancelled_on": [
        "nullable",
        "date"
      ],
      "capital_reduction": [
        "nullable",
        "amount"
      ]
    }
  ],
  "DividendLegalFacts": [
    "object",
    {
      "registered_capital": "amount",
      "source_evidence": "string"
    }
  ],
  "SimplePreferenceRejection": [
    "object",
    {
      "company": "u32",
      "evaluated_on": "date",
      "kind": [
        "ref",
        "SimplePreferenceProposalKind"
      ],
      "detail": "string"
    }
  ],
  "SettlementCycle": [
    "enum",
    {
      "Monthly": null,
      "Quarterly": null,
      "HalfYear": null,
      "Annual": null
    }
  ],
  "TrendSegment": [
    "object",
    {
      "annual_growth_bp": "i32",
      "months": "u8"
    }
  ],
  "ShareLot": [
    "object",
    {
      "id": "string",
      "qty": "canonicalU64",
      "acquired_on": "date",
      "source": [
        "ref",
        "AcquisitionSource"
      ],
      "restriction": [
        "ref",
        "ShareRestriction"
      ]
    }
  ],
  "ShareDayRequest": [
    "object",
    {
      "event_id": "string",
      "day": "date",
      "scope": [
        "ref",
        "MovementScope"
      ],
      "changes": [
        "array",
        [
          "ref",
          "DayNetChange"
        ]
      ]
    }
  ],
  "DisposedLot": [
    "object",
    {
      "holder": [
        "ref",
        "HolderId"
      ],
      "lot": [
        "ref",
        "ShareLot"
      ],
      "disposed_on": "date"
    }
  ],
  "CalendarExchange": [
    "enum",
    {
      "sse": null,
      "szse": null
    }
  ],
  "CashDividendFormula": [
    "enum",
    {
      "StandardCashOnly": null,
      "ExchangeApprovedAdjustment": null
    }
  ],
  "HolderPaymentOutcome": [
    "enum",
    {
      "Paid": [
        "object",
        {
          "holder": [
            "ref",
            "HolderId"
          ],
          "amount": "money"
        }
      ],
      "Failed": [
        "object",
        {
          "holder": [
            "ref",
            "HolderId"
          ],
          "reason": "string"
        }
      ]
    }
  ],
  "TaxAcquisitionSource": [
    "enum",
    {
      "InitialAllocation": [
        "object",
        {
          "evidence": "string"
        }
      ],
      "SecondaryMarket": [
        "object",
        {
          "settlement": "string"
        }
      ],
      "CorporateAction": [
        "object",
        {
          "event": "string"
        }
      ]
    }
  ],
  "TaxShareClass": [
    "enum",
    {
      "PublicMarket": null,
      "StatutoryRestricted": [
        "object",
        {
          "release_on": "date",
          "basis": [
            "ref",
            "StatutoryRestrictedBasis"
          ],
          "qualification_evidence": "string"
        }
      ]
    }
  ],
  "TaxDisposition": [
    "object",
    {
      "lot": [
        "ref",
        "DividendTaxLot"
      ],
      "disposed_on": "date"
    }
  ],
  "ExactDividendTaxAmount": [
    "object",
    {
      "numerator": "nonnegativeI128",
      "denominator": "canonicalU64"
    }
  ],
  "DividendCashReceipt": [
    "object",
    {
      "operation_seq": "u64number",
      "payment_id": "string",
      "paid_on": "date",
      "received_gross": "money",
      "evidence": "string"
    }
  ],
  "StockDistributionKind": [
    "enum",
    {
      "BonusShares": null,
      "CapitalReserveConversion": null
    }
  ],
  "SourceLotAttribution": [
    "enum",
    {
      "SourceLotAttributionPending": null
    }
  ],
  "HolderDistribution": [
    "object",
    {
      "holder": [
        "ref",
        "HolderId"
      ],
      "original_shares": "canonicalU64",
      "whole_shares": "canonicalU64",
      "fractional_numerator": "canonicalU64",
      "original_lots": [
        "array",
        [
          "ref",
          "ShareLot"
        ]
      ]
    }
  ],
  "ShareSplitDirection": [
    "enum",
    {
      "Split": null,
      "Consolidate": null
    }
  ],
  "HolderSplitOutcome": [
    "object",
    {
      "holder": [
        "ref",
        "HolderId"
      ],
      "original_shares": "canonicalU64",
      "new_shares": "canonicalU64",
      "fractional_numerator": "canonicalU64",
      "tie_break_award": "canonicalU64",
      "original_lots": [
        "array",
        [
          "ref",
          "ShareLot"
        ]
      ]
    }
  ],
  "RightsOfferingMode": [
    "enum",
    {
      "RightsToAllShareholders": [
        "object",
        {
          "shares_per_existing_share_micros": "canonicalU64"
        }
      ],
      "DirectedPlacement": [
        "object",
        {
          "targets": [
            "array",
            [
              "ref",
              "DirectedPlacementTarget"
            ]
          ]
        }
      ]
    }
  ],
  "RightsSubscriptionStrategy": [
    "enum",
    {
      "FullByDefault": null,
      "StrategyBased": null
    }
  ],
  "RightsEntitlement": [
    "object",
    {
      "holder": [
        "ref",
        "HolderId"
      ],
      "rights_shares": "canonicalU64",
      "lock_until": [
        "nullable",
        "date"
      ]
    }
  ],
  "SubscriptionOrigin": [
    "enum",
    {
      "Explicit": null,
      "NpcFullByDefault": null
    }
  ],
  "HolderRightsSettlement": [
    "object",
    {
      "holder": [
        "ref",
        "HolderId"
      ],
      "paid_shares": "canonicalU64",
      "paid_amount": "money",
      "waived_shares": "canonicalU64",
      "refunded_amount": "money"
    }
  ],
  "RepurchasePurpose": [
    "enum",
    {
      "ReduceCapital": null,
      "EmployeeIncentive": null,
      "ConvertibleConversion": null,
      "ValueMaintenance": null
    }
  ],
  "RepurchaseCompletionPolicy": [
    "enum",
    {
      "CancelOnCompletion": null
    }
  ],
  "DividendDeclaration": [
    "object",
    {
      "plan_id": "string",
      "approved_on": "date",
      "total_gross": "amount",
      "registered_capital": "amount"
    }
  ],
  "DividendPaymentState": [
    "object",
    {
      "source": "u64number",
      "paid_on": "date",
      "amount": "amount"
    }
  ],
  "SimplePreferenceProposalKind": [
    "enum",
    {
      "CashDividend": null,
      "StockDistribution": null
    }
  ],
  "AcquisitionSource": [
    "enum",
    {
      "InitialAllocation": [
        "object",
        {
          "evidence": "string"
        }
      ],
      "SecondaryMarket": [
        "object",
        {
          "settlement": "string"
        }
      ],
      "CorporateAction": [
        "object",
        {
          "event": "string"
        }
      ]
    }
  ],
  "ShareRestriction": [
    "enum",
    {
      "Unrestricted": null,
      "Restricted": [
        "object",
        {
          "reason": "string",
          "release_on": "date"
        }
      ]
    }
  ],
  "MovementScope": [
    "enum",
    {
      "PublicMarket": null,
      "SettlementDayMarket": [
        "object",
        {
          "binding": [
            "ref",
            "ShareSettlementBinding"
          ]
        }
      ],
      "NonTradingTransfer": [
        "object",
        {
          "basis": "string"
        }
      ],
      "IssuerRepurchaseCancellation": [
        "object",
        {
          "basis": "string"
        }
      ],
      "ShareReDenomination": [
        "object",
        {
          "basis": "string"
        }
      ]
    }
  ],
  "DayNetChange": [
    "object",
    {
      "holder": [
        "ref",
        "HolderId"
      ],
      "change": "canonicalI128",
      "acquisition": [
        "nullable",
        [
          "ref",
          "NetAcquisition"
        ]
      ],
      "consume": [
        "nullable",
        [
          "array",
          [
            "ref",
            "LotConsumption"
          ]
        ]
      ]
    }
  ],
  "StatutoryRestrictedBasis": [
    "enum",
    {
      "FinanceTax2009167": null,
      "FinanceTax201070": null
    }
  ],
  "DirectedPlacementTarget": [
    "enum",
    {
      "NamedHolder": [
        "object",
        {
          "holder": [
            "ref",
            "HolderId"
          ],
          "shares": "canonicalU64",
          "lock_until": [
            "nullable",
            "date"
          ]
        }
      ],
      "OpenPublicSubscription": [
        "object",
        {
          "shares": "canonicalU64"
        }
      ]
    }
  ],
  "ShareSettlementBinding": [
    "object",
    {
      "predecessor": [
        "ref",
        "ShareDayFrontier"
      ],
      "opening": [
        "array",
        [
          "ref",
          "OpeningMovementRef"
        ]
      ],
      "market_unit": [
        "ref",
        "ShareUnitIdentity"
      ],
      "market_source": [
        "ref",
        "ShareMarketSource"
      ],
      "post": [
        "array",
        [
          "ref",
          "SharePostActionRef"
        ]
      ]
    }
  ],
  "NetAcquisition": [
    "object",
    {
      "lot_id": "string",
      "source": [
        "ref",
        "AcquisitionSource"
      ],
      "restriction": [
        "ref",
        "ShareRestriction"
      ]
    }
  ],
  "LotConsumption": [
    "object",
    {
      "lot_id": "string",
      "qty": "canonicalU64"
    }
  ],
  "ShareDayFrontier": [
    "object",
    {
      "settled_on": "date",
      "receipt_count": "canonicalU64",
      "unit": [
        "ref",
        "ShareUnitIdentity"
      ]
    }
  ],
  "OpeningMovementRef": [
    "object",
    {
      "movement_event_id": "string",
      "registration_event_id": "string",
      "source_lots": [
        "array",
        [
          "array",
          "string"
        ]
      ],
      "from_unit": [
        "ref",
        "ShareUnitIdentity"
      ],
      "to_unit": [
        "ref",
        "ShareUnitIdentity"
      ]
    }
  ],
  "ShareUnitIdentity": [
    "enum",
    {
      "Initial": null,
      "ReDenominated": [
        "object",
        {
          "movement_event_id": "string"
        }
      ]
    }
  ],
  "ShareMarketSource": [
    "enum",
    {
      "FinalConfirmations": [
        "object",
        {
          "receipts": [
            "array",
            [
              "ref",
              "ShareConfirmationRef"
            ]
          ]
        }
      ],
      "CalendarClosed": [
        "object",
        {
          "exchange": [
            "ref",
            "CalendarExchange"
          ],
          "policy_digest": "string"
        }
      ]
    }
  ],
  "SharePostActionRef": [
    "enum",
    {
      "Movement": [
        "object",
        {
          "event_id": "string"
        }
      ],
      "UnitMovement": [
        "object",
        {
          "event_id": "string",
          "from_unit": [
            "ref",
            "ShareUnitIdentity"
          ],
          "to_unit": [
            "ref",
            "ShareUnitIdentity"
          ]
        }
      ],
      "CaptureRegistration": [
        "object",
        {
          "event_id": "string"
        }
      ]
    }
  ],
  "ShareConfirmationRef": [
    "object",
    {
      "account": "account",
      "receipt_id": "canonicalU64"
    }
  ]
};

import type { parseCompanySpec } from "./accounting/spec.ts";
import type { parseBooks } from "./accounting/books.ts";
import type { parseClosingRegistry } from "./closing.ts";
import type { parseIncomeTaxPosition } from "./books/industrial.ts";
import type { SimpleConfig } from "../../../types/generated/SimpleConfig.ts";
import type { SimpleFinanceConfig } from "../../../types/generated/SimpleFinanceConfig.ts";
import type { CompanyKind } from "./accounting/spec.ts";

export type CompanySystemWire = { "issuers": Record<string, ReturnType<typeof parseCompanySpec>>; "implementation": CompanyImplementationWire };
export type SimpleFundamentalsWire = { "config": SimpleConfig; "history_start": string; "advanced_through": string; "environment_change_bp": number; "environment_rng": { state: string }; "companies": Record<string, SimpleCompanyStateWire>; "history": (SimpleDisclosureCandidateWire)[] };
export type SessionCorporateActionsWire = { "registries": (ShareRegistryWire)[]; "dividends": (CashDividendBookWire)[]; "dividend_tax_books": (CashDividendTaxBookWire)[]; "flat_withholding_receipts": (FlatWithholdingReceiptWire)[]; "stock_distributions": (StockDistributionBookWire)[]; "share_splits": (ShareSplitBookWire)[]; "rights_offerings": (RightsOfferingBookWire)[]; "rights_subscription_queue": (QueuedRightsSubscriptionWire)[]; "rejected_rights_subscriptions": (RejectedRightsSubscriptionWire)[]; "issuer_repurchases": (IssuerRepurchaseBookWire)[]; "repurchase_execution_bindings": (RepurchaseExecutionBindingWire)[]; "account_gross_receipts": (AccountDividendGrossReceiptWire)[]; "external_receipts": (ExternalDividendReceiptWire)[]; "applied_ex_reference_groups": (AppliedExReferenceGroupWire)[] };
export type CompanyImplementationWire = { "mode": "Simple"; "state": SimpleFundamentalsWire };
export type SimpleCompanyStateWire = { "generation": PeriodGenerationStateWire; "finance": SimpleFinanceStateWire; "pending_restart": ([string, string] | null); "preference_ledger": SimplePreferenceLedgerWire };
export type SimpleDisclosureCandidateWire = { "company": number; "period_start": string; "period_end": string; "amounts": PeriodAmountsWire; "explanation": PeriodChangeExplanationWire };
export type ShareRegistryWire = { "stock": string; "issuer": number; "issued_shares": string; "issuer_repurchase_account": (IssuerRepurchaseAccountFactsWire | null); "settled_on": string; "holdings": (ShareHoldingWire)[]; "receipts": (ShareDayReceiptWire)[]; "registrations": (RegistrationSnapshotWire)[]; "pending_opening": (PendingOpeningWire | null) };
export type CashDividendBookWire = { "plan": CashDividendPlanWire; "status": CashDividendStatusWire; "registration": (RegistrationSnapshotWire | null); "entitlements": (CashDividendEntitlementWire)[]; "paid": ([HolderIdWire, string])[]; "failures": ([HolderIdWire, string])[]; "payments": (CashDividendPaymentReceiptWire)[] };
export type CashDividendTaxBookWire = { "operation_seq": number; "account": string; "stock": string; "profile": DividendTaxProfileWire; "opened_on": string; "opening_lots": (DividendTaxLotWire)[]; "settled_on": string; "lots": (DividendTaxLotWire)[]; "days": (TaxDayReceiptWire)[]; "dividends": (RegisteredTaxDividendWire)[]; "collections": (TaxCollectionReceiptWire)[]; "redenominations": (TaxRedenominationReceiptWire)[]; "stock_bonus_bases": (StockBonusTaxBasisWire)[] };
export type FlatWithholdingReceiptWire = { "payment_id": string; "plan_id": string; "account": string; "paid_on": string; "gross": string; "rate_bp": number; "withheld": string };
export type StockDistributionBookWire = { "plan": StockDistributionEventPlanWire; "status": StockDistributionStatusWire; "registration": (RegistrationSnapshotWire | null); "receipt": (StockDistributionReceiptWire | null); "credited_on": (string | null) };
export type ShareSplitBookWire = { "plan": ShareSplitEventPlanWire; "status": ShareSplitStatusWire; "registration": (RegistrationSnapshotWire | null); "receipt": (ShareSplitReceiptWire | null); "settled_on": (string | null) };
export type RightsOfferingBookWire = { "plan": RightsOfferingEventPlanWire; "status": RightsOfferingStatusWire; "registration": (RegistrationSnapshotWire | null); "entitlement": (RightsEntitlementReceiptWire | null); "subscriptions": (RightsSubscriptionRecordWire)[]; "closed_on": (string | null); "settlement": (RightsSettlementReceiptWire | null); "credited_on": (string | null) };
export type QueuedRightsSubscriptionWire = { "event_id": string; "requested_shares": string; "submitted_on": string; "account": string };
export type RejectedRightsSubscriptionWire = { "event_id": string; "account": string; "requested_shares": string; "submitted_on": string; "rejected_on": string; "reason": string };
export type IssuerRepurchaseBookWire = { "plan": IssuerRepurchasePlanWire; "status": IssuerRepurchaseStatusWire; "fills": (RepurchaseFillRecordWire)[]; "share_unit_adjustments": (ShareUnitAdjustmentWire)[]; "completed_on": (string | null); "withdrawn_remainder": (string | null); "cancelled_on": (string | null); "cancelled_shares": string; "last_order_day": (string | null) };
export type AccountDividendGrossReceiptWire = { "payment_id": string; "plan_id": string; "account": string; "paid_on": string; "gross": string; "tax_status": DividendTaxStatusWire };
export type ExternalDividendReceiptWire = { "payment_id": string; "plan_id": string; "holder": HolderIdWire; "paid_on": string; "gross": string; "tax_status": DividendTaxStatusWire };
export type AppliedExReferenceGroupWire = { "date": string; "stock": string; "cash_plan_ids": (string)[]; "stock_event_ids": (string)[]; "rights_event_ids": (string)[]; "split_event_ids": (string)[]; "pre_ex_close": string; "reference": ExReferencePriceWire };
export type PeriodGenerationStateWire = { "amounts": PeriodAmountsWire; "rng": { state: string }; "revenue_trend": AnnualTrendStateWire; "fixed_expense_trend": AnnualTrendStateWire; "variable_expense_trend": (AnnualTrendStateWire | null) };
export type SimpleFinanceStateWire = { "company": number; "kind": CompanyKind; "config": SimpleFinanceConfig; "books": ReturnType<typeof parseBooks>; "closing": ReturnType<typeof parseClosingRegistry>; "opening_date": string; "as_of": string; "last_month": string; "next_event_id": string; "income_tax_position": ReturnType<typeof parseIncomeTaxPosition>; "recognized_periods": ([string, string])[]; "dividends": Record<string, DividendPlanStateWire>; "stock_distributions": Record<string, StockDistributionFinanceFactWire>; "share_splits": Record<string, ShareSplitFinanceFactWire>; "rights_offerings": Record<string, RightsOfferingFinanceFactWire>; "issuer_repurchases": Record<string, IssuerRepurchaseFinanceFactWire>; "legal_facts": (DividendLegalFactsWire | null); "cash_book": string; "investment_book": string };
export type SimplePreferenceLedgerWire = { "rejections": (SimplePreferenceRejectionWire)[] };
export type PeriodAmountsWire = { "revenue": string; "fixed_expense": string; "variable_expense": string };
export type PeriodChangeExplanationWire = { "previous": PeriodAmountsWire; "cycle": SettlementCycleWire; "environment_change_bp": number; "demand_contribution_bp": number; "revenue_segments": (TrendSegmentWire)[]; "fixed_expense_segments": (TrendSegmentWire)[]; "variable_expense_segments": (TrendSegmentWire)[]; "revenue_noise_bp": number; "fixed_expense_noise_bp": number; "variable_expense_noise_bp": number; "restart_revenue": (string | null); "restart_source": (string | null) };
export type IssuerRepurchaseAccountFactsWire = { "account_reference": string; "source_evidence": string; "established_on": string };
export type ShareHoldingWire = { "holder": HolderIdWire; "lots": (ShareLotWire)[] };
export type ShareDayReceiptWire = { "request": ShareDayRequestWire; "disposals": (DisposedLotWire)[] };
export type RegistrationSnapshotWire = { "event_id": string; "stock": string; "issuer": number; "registered_on": string; "issued_shares": string; "settled_receipts": string; "issuer_repurchase_account": (IssuerRepurchaseAccountFactsWire | null); "holdings": (ShareHoldingWire)[] };
export type CashDividendPlanWire = { "plan_id": string; "issuer": number; "stock": string; "exchange": CalendarExchangeWire; "formula": CashDividendFormulaWire; "approved_on": string; "announced_on": string; "registered_on": string; "ex_dividend_on": string; "payable_on": string; "gross_per_share": string; "distributable_amount": string };
export type CashDividendStatusWire = "Approved" | "Announced" | "Registered" | "Payable" | "PartiallyPaid" | "Paid";
export type CashDividendEntitlementWire = { "holder": HolderIdWire; "shares": string; "gross": string };
export type HolderIdWire = { "Account": string } | { "External": string } | "IssuerTreasury";
export type CashDividendPaymentReceiptWire = { "payment_id": string; "paid_on": string; "within_six_month_deadline": boolean; "outcomes": (HolderPaymentOutcomeWire)[] };
export type DividendTaxProfileWire = "IndividualPublicMarket" | "ResidentEnterprise" | "SecuritiesFund" | "NonResident";
export type DividendTaxLotWire = { "id": string; "qty": string; "acquired_on": string; "source": TaxAcquisitionSourceWire; "class": TaxShareClassWire };
export type TaxDayReceiptWire = { "operation_seq": number; "event_id": string; "day": string; "net_change": string; "acquisition": (DividendTaxLotWire | null); "dispositions": (TaxDispositionWire)[] };
export type RegisteredTaxDividendWire = { "operation_seq": number; "event_id": string; "registered_on": string; "per_share": ExactDividendTaxAmountWire; "lots": (DividendTaxLotWire)[]; "payments": (DividendCashReceiptWire)[] };
export type TaxCollectionReceiptWire = { "operation_seq": number; "event_id": string; "day": string; "available_cash": string; "collected": string; "remaining_cash": string; "outstanding": ExactDividendTaxAmountWire; "needs_funds": boolean };
export type TaxRedenominationReceiptWire = { "operation_seq": number; "event_id": string; "day": string; "net_change": string; "removed": (TaxDispositionWire)[] };
export type StockBonusTaxBasisWire = { "operation_seq": number; "event_id": string; "credited_on": string; "lot_id": string; "qty": string; "par_value_per_share": string };
export type StockDistributionEventPlanWire = { "event_id": string; "approval_reference": string; "issuer": number; "stock": string; "exchange": CalendarExchangeWire; "kind": StockDistributionKindWire; "approved_on": string; "announced_on": string; "registered_on": string; "ex_rights_on": string; "shares_per_existing_share_micros": string; "approved_total_new_shares": string };
export type StockDistributionStatusWire = "Approved" | "Announced" | "Registered" | "Credited";
export type StockDistributionReceiptWire = { "event_id": string; "approval_reference": string; "registration_event_id": string; "stock": string; "issuer": number; "registered_on": string; "issued_shares_before": string; "issuer_treasury_shares_excluded": string; "kind": StockDistributionKindWire; "shares_per_existing_share_micros": string; "approved_total_new_shares": string; "tie_break_seed": string; "source_lot_attribution": SourceLotAttributionWire; "holders": (HolderDistributionWire)[] };
export type ShareSplitEventPlanWire = { "event_id": string; "approval_reference": string; "issuer": number; "stock": string; "exchange": CalendarExchangeWire; "direction": ShareSplitDirectionWire; "ratio": string; "approved_on": string; "announced_on": string; "registered_on": string; "ex_rights_on": string };
export type ShareSplitStatusWire = "Approved" | "Announced" | "Registered" | "Settled";
export type ShareSplitReceiptWire = { "event_id": string; "approval_reference": string; "registration_event_id": string; "stock": string; "issuer": number; "registered_on": string; "direction": ShareSplitDirectionWire; "ratio": string; "issued_shares_before": string; "issued_shares_after": string; "tie_break_seed": string; "holders": (HolderSplitOutcomeWire)[] };
export type RightsOfferingEventPlanWire = { "event_id": string; "approval_reference": string; "issuer": number; "stock": string; "exchange": CalendarExchangeWire; "approved_on": string; "announced_on": string; "registered_on": string; "payment_start_on": string; "payment_deadline_on": string; "ex_rights_on": string; "settlement_on": string; "listing_on": string; "price_per_share": string; "mode": RightsOfferingModeWire; "npc_subscription_strategy": RightsSubscriptionStrategyWire };
export type RightsOfferingStatusWire = "Approved" | "Announced" | "Entitled" | "Closed" | "Settled";
export type RightsEntitlementReceiptWire = { "event_id": string; "approval_reference": string; "registration_event_id": string; "stock": string; "issuer": number; "registered_on": string; "issued_shares_before": string; "issuer_treasury_shares_excluded": string; "price_per_share": string; "mode": RightsOfferingModeWire; "planned_total_rights_shares": string; "tie_break_seed": string; "entitlements": (RightsEntitlementWire)[]; "open_subscription_shares": string };
export type RightsSubscriptionRecordWire = { "holder": HolderIdWire; "requested_shares": string; "price_per_share": string; "submitted_on": string; "origin": SubscriptionOriginWire; "paid_shares": string; "paid_amount": string; "waived_shares": string };
export type RightsSettlementReceiptWire = { "event_id": string; "settlement_on": string; "failed": boolean; "total_paid_shares": string; "total_paid_amount": string; "refunded_total": string; "holders": (HolderRightsSettlementWire)[] };
export type IssuerRepurchasePlanWire = { "event_id": string; "approval_reference": string; "issuer": number; "stock": string; "exchange": CalendarExchangeWire; "approved_on": string; "announced_on": string; "window_start_on": string; "window_deadline_on": string; "price_cap_per_share": string; "total_budget": string; "max_shares": string; "purpose": RepurchasePurposeWire; "completion_policy": RepurchaseCompletionPolicyWire; "approval_unit_event_id": (string | null) };
export type IssuerRepurchaseStatusWire = "Approved" | "Announced" | "Executing" | "Completed" | "Cancelled";
export type RepurchaseFillRecordWire = { "stock": string; "day": string; "shares": string; "gross": string; "fees": string };
export type ShareUnitAdjustmentWire = { "split_event_id": string; "shares_before": string; "shares_after": string };
export type DividendTaxStatusWire = "FlatWithholding" | "IndividualPublicMarket" | "TreatmentNotConfigured";
export type ExReferencePriceWire = { "ex_date": string; "reference_price": string };
export type AnnualTrendStateWire = { "annual_growth_bp": number; "remaining_months": number };
export type DividendPlanStateWire = { "declaration": DividendDeclarationWire; "declaration_source": number; "reserve": string; "reserve_basis_year": (number | null); "payments": Record<string, DividendPaymentStateWire> };
export type StockDistributionFinanceFactWire = { "event_id": string; "approval_reference": string; "kind": StockDistributionKindWire; "approved_on": string; "new_shares": string; "par_value_per_share": string; "capital_increase": string; "registered_capital_at_approval": string; "credited_on": (string | null); "statutory_reserve": string; "reserve_basis_year": (number | null) };
export type ShareSplitFinanceFactWire = { "event_id": string; "approval_reference": string; "direction": ShareSplitDirectionWire; "approved_on": string; "ratio": string; "par_value_before": string; "par_value_after": string; "registered_capital_at_approval": string; "issued_shares_before": string; "issued_shares_after": string; "destroyed_shares": string; "registered_capital_reduction": string; "settled_on": (string | null) };
export type RightsOfferingFinanceFactWire = { "event_id": string; "approval_reference": string; "approved_on": string; "price_per_share": string; "par_value_per_share": string; "registered_capital_at_approval": string; "settled_on": (string | null); "issued_shares": string; "proceeds": (string | null); "capital_increase": (string | null) };
export type IssuerRepurchaseFinanceFactWire = { "event_id": string; "approval_reference": string; "approved_on": string; "synthetic_funding": string; "purpose": RepurchasePurposeWire; "spent": (string | null); "withdrawn_remainder": (string | null); "completed_on": (string | null); "cancelled_shares": string; "cancelled_on": (string | null); "capital_reduction": (string | null) };
export type DividendLegalFactsWire = { "registered_capital": string; "source_evidence": string };
export type SimplePreferenceRejectionWire = { "company": number; "evaluated_on": string; "kind": SimplePreferenceProposalKindWire; "detail": string };
export type SettlementCycleWire = "Monthly" | "Quarterly" | "HalfYear" | "Annual";
export type TrendSegmentWire = { "annual_growth_bp": number; "months": number };
export type ShareLotWire = { "id": string; "qty": string; "acquired_on": string; "source": AcquisitionSourceWire; "restriction": ShareRestrictionWire };
export type ShareDayRequestWire = { "event_id": string; "day": string; "scope": MovementScopeWire; "changes": (DayNetChangeWire)[] };
export type DisposedLotWire = { "holder": HolderIdWire; "lot": ShareLotWire; "disposed_on": string };
export type CalendarExchangeWire = "sse" | "szse";
export type CashDividendFormulaWire = "StandardCashOnly" | "ExchangeApprovedAdjustment";
export type HolderPaymentOutcomeWire = { "Paid": { "holder": HolderIdWire; "amount": string } } | { "Failed": { "holder": HolderIdWire; "reason": string } };
export type TaxAcquisitionSourceWire = { "InitialAllocation": { "evidence": string } } | { "SecondaryMarket": { "settlement": string } } | { "CorporateAction": { "event": string } };
export type TaxShareClassWire = "PublicMarket" | { "StatutoryRestricted": { "release_on": string; "basis": StatutoryRestrictedBasisWire; "qualification_evidence": string } };
export type TaxDispositionWire = { "lot": DividendTaxLotWire; "disposed_on": string };
export type ExactDividendTaxAmountWire = { "numerator": string; "denominator": string };
export type DividendCashReceiptWire = { "operation_seq": number; "payment_id": string; "paid_on": string; "received_gross": string; "evidence": string };
export type StockDistributionKindWire = "BonusShares" | "CapitalReserveConversion";
export type SourceLotAttributionWire = "SourceLotAttributionPending";
export type HolderDistributionWire = { "holder": HolderIdWire; "original_shares": string; "whole_shares": string; "fractional_numerator": string; "original_lots": (ShareLotWire)[] };
export type ShareSplitDirectionWire = "Split" | "Consolidate";
export type HolderSplitOutcomeWire = { "holder": HolderIdWire; "original_shares": string; "new_shares": string; "fractional_numerator": string; "tie_break_award": string; "original_lots": (ShareLotWire)[] };
export type RightsOfferingModeWire = { "RightsToAllShareholders": { "shares_per_existing_share_micros": string } } | { "DirectedPlacement": { "targets": (DirectedPlacementTargetWire)[] } };
export type RightsSubscriptionStrategyWire = "FullByDefault" | "StrategyBased";
export type RightsEntitlementWire = { "holder": HolderIdWire; "rights_shares": string; "lock_until": (string | null) };
export type SubscriptionOriginWire = "Explicit" | "NpcFullByDefault";
export type HolderRightsSettlementWire = { "holder": HolderIdWire; "paid_shares": string; "paid_amount": string; "waived_shares": string; "refunded_amount": string };
export type RepurchasePurposeWire = "ReduceCapital" | "EmployeeIncentive" | "ConvertibleConversion" | "ValueMaintenance";
export type RepurchaseCompletionPolicyWire = "CancelOnCompletion";
export type DividendDeclarationWire = { "plan_id": string; "approved_on": string; "total_gross": string; "registered_capital": string };
export type DividendPaymentStateWire = { "source": number; "paid_on": string; "amount": string };
export type SimplePreferenceProposalKindWire = "CashDividend" | "StockDistribution";
export type AcquisitionSourceWire = { "InitialAllocation": { "evidence": string } } | { "SecondaryMarket": { "settlement": string } } | { "CorporateAction": { "event": string } };
export type ShareRestrictionWire = "Unrestricted" | { "Restricted": { "reason": string; "release_on": string } };
export type MovementScopeWire = "PublicMarket" | { "SettlementDayMarket": { "binding": ShareSettlementBindingWire } } | { "NonTradingTransfer": { "basis": string } } | { "IssuerRepurchaseCancellation": { "basis": string } } | { "ShareReDenomination": { "basis": string } };
export type DayNetChangeWire = { "holder": HolderIdWire; "change": string; "acquisition": (NetAcquisitionWire | null); "consume": ((LotConsumptionWire)[] | null) };
export type StatutoryRestrictedBasisWire = "FinanceTax2009167" | "FinanceTax201070";
export type DirectedPlacementTargetWire = { "NamedHolder": { "holder": HolderIdWire; "shares": string; "lock_until": (string | null) } } | { "OpenPublicSubscription": { "shares": string } };
export type ShareSettlementBindingWire = { "predecessor": ShareDayFrontierWire; "opening": (OpeningMovementRefWire)[]; "market_unit": ShareUnitIdentityWire; "market_source": ShareMarketSourceWire; "post": (SharePostActionRefWire)[] };
export type NetAcquisitionWire = { "lot_id": string; "source": AcquisitionSourceWire; "restriction": ShareRestrictionWire };
export type LotConsumptionWire = { "lot_id": string; "qty": string };
export type ShareDayFrontierWire = { "settled_on": string; "receipt_count": string; "unit": ShareUnitIdentityWire };
export type OpeningMovementRefWire = { "movement_event_id": string; "registration_event_id": string; "source_lots": ((string)[])[]; "from_unit": ShareUnitIdentityWire; "to_unit": ShareUnitIdentityWire };
export type ShareUnitIdentityWire = "Initial" | { "ReDenominated": { "movement_event_id": string } };
export type ShareMarketSourceWire = { "FinalConfirmations": { "receipts": (ShareConfirmationRefWire)[] } } | { "CalendarClosed": { "exchange": CalendarExchangeWire; "policy_digest": string } };
export type SharePostActionRefWire = { "Movement": { "event_id": string } } | { "UnitMovement": { "event_id": string; "from_unit": ShareUnitIdentityWire; "to_unit": ShareUnitIdentityWire } } | { "CaptureRegistration": { "event_id": string } };
export type ShareConfirmationRefWire = { "account": string; "receipt_id": string };
export type RepurchaseExecutionBindingWire = { "event_id": string; "issuer": number; "stock": string; "account": string; "plan_id": PlanIdWire; "funding_committed": boolean; "funding_amount": string; "applied_fact_cursor": string; "closing_state": RepurchaseBindingClosingStateWire };
export type PlanIdWire = { "owner": string; "sequence": string };
export type RepurchaseBindingClosingStateWire = "Active" | "Draining" | "Closed";

export type PendingOpeningWire = { "predecessor": ShareDayFrontierWire; "opening": FrozenOpeningMovementWire };
export type FrozenOpeningMovementWire = { "reference": OpeningMovementRefWire; "request": ShareDayRequestWire };
