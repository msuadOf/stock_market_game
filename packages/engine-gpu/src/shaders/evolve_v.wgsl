// V 演化 compute shader（整数定点，跨 GPU 厂商确定性）
//
// 每个工作组处理一只股票的 V 演化。
// 输入：v_cents(i32), mean_cents(i32), mean_reversion_bp(i32, 基点), volatility_bp(i32, 基点), seed(u32)
// 输出：new_v_cents(i32), error(i32, 0=ok, 1=multiplier<=0, 2=new_v<=0)
//
// 算法（等价 CPU 版 market.rs evolve_v）：
//   gap = (mean - v) / v           （整数除法 → 用定点：gap_bp = (mean - v) * 10000 / v）
//   drift = mean_reversion * gap + volatility * z    （z ∈ [-1,1]，由 hash(seed) 生成）
//   multiplier = 10000 + drift      （10000 = 1.0 的定点表示）
//   new_v = v * multiplier / 10000  （整数除法 + half-to-even 舍入）

const BP: i32 = 10000;  // 1.0 = 10000 basis points

fn hash_u32(seed: u32) -> u32 {
    var x = seed;
    x = x ^ (x >> 16u);
    x = x * 0x7feb352du;
    x = x ^ (x >> 15u);
    x = x * 0x846ca68bu;
    x = x ^ (x >> 16u);
    return x;
}

// 将 u32 映射到 [-10000, 10000]（即 [-1.0, 1.0] 的定点表示）
fn hash_to_bp(seed: u32) -> i32 {
    let h = hash_u32(seed);
    // h ∈ [0, 4294967295]，映射到 [-BP, BP]
    return i32(h % (2u * BP + 1u)) - BP;
}

// 整数银行家舍入（half-to-even）：n / d → 最近的偶数
fn round_half_to_even(n: i32, d: i32) -> i32 {
    if d == 0 { return 0; }
    let floor_val = n / d;
    let rem = n - floor_val * d;
    let twice_rem = 2 * rem;
    if twice_rem < d {
        return floor_val;
    } else if twice_rem > d {
        return floor_val + 1;
    } else {
        // 恰好半：取偶数
        if ((floor_val % 2) == 0) {
            return floor_val;
        } else {
            return floor_val + 1;
        }
    }
}

@group(0) @binding(0) var<storage, read> input: array<VInput>;
@group(0) @binding(1) var<storage, read_write> output: array<VOutput>;

struct VInput {
    v_cents: i32,
    mean_cents: i32,
    mean_reversion_bp: i32,
    volatility_bp: i32,
    seed_hi: u32,
    seed_lo: u32,
    _pad0: u32,
    _pad1: u32,
}

struct VOutput {
    new_v_cents: i32,
    error: i32,
    _pad0: u32,
    _pad1: u32,
}

@compute @workgroup_size(1)
fn evolve_v_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx >= arrayCount(input) {
        return;
    }

    let data = input[idx];
    var result: VOutput;

    if data.v_cents <= 0 || data.mean_cents <= 0 {
        result.new_v_cents = data.v_cents;
        result.error = 2;  // v <= 0
        output[idx] = result;
        return;
    }

    // gap_bp = (mean - v) * BP / v （整数除法）
    let gap_bp = (data.mean_cents - data.v_cents) * BP / data.v_cents;

    // z_bp ∈ [-BP, BP]（定点 [-1.0, 1.0]）
    let z_bp = hash_to_bp(data.seed_lo);

    // drift_bp = mean_reversion * gap / BP + volatility * z / BP
    let drift_bp = data.mean_reversion_bp * gap_bp / BP + data.volatility_bp * z_bp / BP;

    // multiplier_bp = BP + drift
    let multiplier_bp = BP + drift_bp;

    if multiplier_bp <= 0 {
        result.new_v_cents = data.v_cents;
        result.error = 1;  // multiplier <= 0
        output[idx] = result;
        return;
    }

    // new_v = v * multiplier / BP（银行家舍入）
    let raw = data.v_cents * multiplier_bp;
    let new_v = round_half_to_even(raw, BP);

    if new_v <= 0 {
        result.new_v_cents = data.v_cents;
        result.error = 2;
        output[idx] = result;
        return;
    }

    result.new_v_cents = new_v;
    result.error = 0;
    output[idx] = result;
}
