use engine::{
    create_backend, ComputeBackend, ComputeError, ComputeMode, CpuBackend, Market, Money,
    StockCode, VParams,
};

fn market(code: &str, value: i64) -> Market {
    Market::new(
        StockCode(code.to_string()),
        Money::from_cents(100),
        0.10,
        Money::from_cents(value),
        Money::from_cents(1),
    )
    .unwrap()
}

#[test]
fn cpu_value_evolution_validates_parallel_input_lengths() {
    let mut markets = vec![market("A", 100)];
    let error = CpuBackend
        .evolve_v_all(
            &mut markets,
            &VParams {
                long_run_mean: Money::from_cents(100),
                mean_reversion: 0.1,
                volatility: 0.0,
            },
            &[],
        )
        .unwrap_err();
    assert!(matches!(error, ComputeError::InvalidInput(_)));
}

#[test]
fn cpu_value_evolution_is_atomic_when_one_market_fails() {
    let mut markets = vec![market("A", 1), market("B", 1_000)];
    let before: Vec<_> = markets.iter().map(Market::fundamental_value).collect();
    let error = CpuBackend
        .evolve_v_all(
            &mut markets,
            &VParams {
                long_run_mean: Money::from_cents(1),
                mean_reversion: 2.0,
                volatility: 0.0,
            },
            &[1, 2],
        )
        .unwrap_err();
    assert!(matches!(
        error,
        ComputeError::MarketEvolution { index: 1, .. }
    ));
    assert_eq!(
        markets
            .iter()
            .map(Market::fundamental_value)
            .collect::<Vec<_>>(),
        before
    );
}

#[test]
fn backend_selection_never_silently_falls_back_from_explicit_gpu() {
    assert!(create_backend(&ComputeMode::Cpu).is_ok());
    assert!(create_backend(&ComputeMode::Auto).is_ok());
    assert!(matches!(
        create_backend(&ComputeMode::Gpu),
        Err(ComputeError::BackendUnavailable(_))
    ));
}
