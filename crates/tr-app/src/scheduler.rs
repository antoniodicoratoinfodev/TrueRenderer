//! Bounded deterministic neighborhood selection, independent of the GUI.
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
    while result.len() < limit.min(64) && (before.is_some() || after.is_some()) {
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
        assert_eq!(prefetch_indices(1000, 500..=500, 1, 1000).len(), 64);
        assert_eq!(prefetch_delay_ms(&[]), 400);
        assert_eq!(prefetch_delay_ms(&[2, 4, 8]), 100);
        assert_eq!(prefetch_delay_ms(&[100, 200, 120]), 250);
        assert_eq!(prefetch_delay_ms(&[u64::MAX]), 500);
    }
}
