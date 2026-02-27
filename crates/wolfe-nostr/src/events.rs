use nostr_sdk::prelude::*;

/// Build a block announcement event (kind 33333, parameterized replaceable).
///
/// Published when a new block is validated by the consensus engine.
pub fn block_announcement(
    height: u64,
    hash: &str,
    timestamp: u64,
    tx_count: usize,
    size: usize,
    network: &str,
) -> EventBuilder {
    let content = serde_json::json!({
        "height": height,
        "hash": hash,
        "timestamp": timestamp,
        "tx_count": tx_count,
        "size": size,
    });

    EventBuilder::new(Kind::Custom(33333), content.to_string())
        .tag(Tag::identifier("block"))
        .tag(Tag::custom(
            TagKind::custom("t"),
            vec!["bitcoin"],
        ))
        .tag(Tag::custom(
            TagKind::custom("t"),
            vec!["block"],
        ))
        .tag(Tag::custom(
            TagKind::custom("height"),
            vec![&height.to_string()],
        ))
        .tag(Tag::custom(
            TagKind::custom("network"),
            vec![network],
        ))
}

/// Build a mempool fee oracle event (kind 33334, parameterized replaceable).
///
/// Published periodically with current mempool fee statistics.
pub fn mempool_fee_oracle(
    size: usize,
    bytes: usize,
    min_fee_rate: f64,
    fee_buckets: &[(f64, usize)], // (fee_rate_sat_vb, tx_count)
    network: &str,
) -> EventBuilder {
    let buckets: Vec<serde_json::Value> = fee_buckets
        .iter()
        .map(|(rate, count)| {
            serde_json::json!({
                "fee_rate": rate,
                "count": count,
            })
        })
        .collect();

    let content = serde_json::json!({
        "size": size,
        "bytes": bytes,
        "min_fee_rate": min_fee_rate,
        "fee_histogram": buckets,
    });

    EventBuilder::new(Kind::Custom(33334), content.to_string())
        .tag(Tag::identifier("mempool-fees"))
        .tag(Tag::custom(
            TagKind::custom("t"),
            vec!["bitcoin"],
        ))
        .tag(Tag::custom(
            TagKind::custom("t"),
            vec!["mempool"],
        ))
        .tag(Tag::custom(
            TagKind::custom("t"),
            vec!["fees"],
        ))
        .tag(Tag::custom(
            TagKind::custom("network"),
            vec![network],
        ))
}
