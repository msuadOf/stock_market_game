include!("../../packages/engine/src/indicators.rs");

#[test]
fn measure_sequential_and_parallel_components() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build()
        .unwrap();
    pool.install(|| {
        for count in [64, 2048, 8192] {
            let prices: Vec<_> = (0..count).map(|index| 10.0 + (index % 17) as f64).collect();
            let candles: Vec<_> = prices
                .iter()
                .map(|price| OhlcBar {
                    high: price + 1.0,
                    low: price - 1.0,
                    close: *price,
                })
                .collect();
            let started = std::time::Instant::now();
            for _ in 0..64 {
                std::hint::black_box((macd(&prices), kdj(&prices), kdj_ohlc(&candles)));
            }
            let sequential = started.elapsed();
            let started = std::time::Instant::now();
            for _ in 0..64 {
                std::hint::black_box(rayon::join(
                    || macd(&prices),
                    || rayon::join(|| kdj(&prices), || kdj_ohlc(&candles)),
                ));
            }
            eprintln!(
                "samples={count}, iterations=64, sequential_us={}, parallel_us={}",
                sequential.as_micros(),
                started.elapsed().as_micros()
            );
        }
    });
}
