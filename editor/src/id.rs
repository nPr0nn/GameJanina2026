//! Random short-ID generation for new shapes.

/// Generate a short random-looking 6-character lowercase alphanumeric ID.
/// Uses a global atomic counter mixed with sub-second time so IDs remain
/// unique even when called many times in rapid succession.
pub(crate) fn random_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    // Mix with a multiplicative hash so closely-timed calls look different.
    let mut h = nanos.wrapping_add(seq.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1));
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    let charset: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut id = String::with_capacity(6);
    let mut val = h;
    for _ in 0..6 {
        id.push(charset[(val % 36) as usize] as char);
        val /= 36;
    }
    id
}
