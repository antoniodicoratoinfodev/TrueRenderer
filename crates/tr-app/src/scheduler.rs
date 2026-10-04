//! Bounded deterministic neighborhood selection, independent of the GUI.

/// Byte-bounded, gradually expanding lookahead; no fixed photo count setting.
/// Pending work is bounded separately by the caller's two admission slots.
pub fn automatic_prefetch_count(
    reusable: u64,
    per_image: u64,
    grid: bool,
    idle_ms: u64,
    battery_saver: bool,
) -> usize {
    let capacity = (reusable / per_image.max(1)).saturating_sub(1).min(4096);
    let initial = if grid { 32 } else { 4 };
    let horizon = initial + idle_ms / 100 * initial;
    let horizon = if battery_saver { initial } else { horizon };
    capacity.min(horizon) as usize
}

use std::ops::RangeInclusive;

/// Outside the visible span only: a jump never visits intermediate positions.
/// Favor the current direction, while keeping a smaller opposite neighborhood.
pub fn prefetch_indices(
    length: usize,
    visible: RangeInclusive<usize>,
    direction: isize,
    limit: usize,
) -> Vec<usize> {
    if length == 0 || visible.is_empty() || *visible.end() >= length {
        return Vec::new();
    }
    let mut before = visible.start().checked_sub(1);
    let mut after = (*visible.end() + 1 < length).then_some(*visible.end() + 1);
    let mut result = Vec::new();
    while result.len() < limit.min(4096) && (before.is_some() || after.is_some()) {
        let forward = (result.len() % 3 != 1) == (direction >= 0);
        let next = if (forward && after.is_some()) || before.is_none() {
            let next = after.take().unwrap();
            after = (next + 1 < length).then_some(next + 1);
            next
        } else {
            let next = before.take().unwrap();
            before = next.checked_sub(1);
            next
        };
        result.push(next);
    }
    result
}

/// Scheduling heuristic, not a qualified end-to-end latency percentile.
pub fn prefetch_delay_ms(recent: &[u64]) -> u64 {
    if recent.is_empty() {
        return 400;
    }
    let mut sorted = recent.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() - 1 - sorted.len() / 20]
        .saturating_add(50)
        .clamp(100, 500)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_lookahead_grows_with_idle_capacity_and_yields_to_pressure_or_battery() {
        assert_eq!(automatic_prefetch_count(1000, 100, false, 0, false), 4);
        assert_eq!(automatic_prefetch_count(1000, 100, false, 1000, false), 9);
        assert_eq!(automatic_prefetch_count(200, 100, false, 1000, false), 1);
        assert_eq!(automatic_prefetch_count(0, 100, false, 1000, false), 0);
        assert_eq!(automatic_prefetch_count(1000, 100, false, 1000, true), 4);
        assert!(automatic_prefetch_count(1000, 1, true, 1000, false) > 32);
        assert_eq!(prefetch_indices(20, 10..=10, -1, 4), [9, 11, 8, 7]);
    }
    #[test]
    fn direction_reversal_jump_edges_and_descriptor_limit() {
        assert_eq!(prefetch_indices(1000, 10..=10, 1, 4), [11, 9, 12, 13]);
        assert_eq!(
            prefetch_indices(1000, 900..=900, -1, 4),
            [899, 901, 898, 897]
        );
        assert_eq!(prefetch_indices(20, 4..=9, 1, 3), [10, 3, 11]);
        assert_eq!(prefetch_indices(3, 0..=0, -1, 10), [1, 2]);
        assert_eq!(prefetch_indices(3, 2..=2, 1, 10), [1, 0]);
        assert!(prefetch_indices(0, 0..=0, 1, 10).is_empty());
        assert_eq!(prefetch_indices(1000, 500..=500, 1, 1000).len(), 999);
        assert_eq!(prefetch_delay_ms(&[]), 400);
        assert_eq!(prefetch_delay_ms(&[2, 4, 8]), 100);
        assert_eq!(prefetch_delay_ms(&[100, 200, 120]), 250);
        assert_eq!(prefetch_delay_ms(&[u64::MAX]), 500);
    }
}
