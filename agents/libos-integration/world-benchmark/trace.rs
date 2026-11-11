/// 对每个实际序列化字段单独摘要，保留字节长度和顺序；仅用于缩小差异范围。
pub fn summarize(fields: &[Vec<u8>]) -> Vec<(usize, String)> {
    fields.iter().map(|field| {
        let mut hash = 0xcbf29ce484222325_u64;
        for byte in field {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
        (field.len(), format!("{hash:016x}"))
    }).collect()
}

/// 提取指定原始字段，保留 JSON 字节的精确文本以分析差异。
pub fn sample(fields: &[Vec<u8>], index: usize) -> Result<&str, String> {
    let field = fields.get(index).ok_or_else(|| format!("字段索引越界：{index}"))?;
    std::str::from_utf8(field).map_err(|error| format!("字段不是 UTF-8：{error}"))
}
