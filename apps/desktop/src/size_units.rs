//! Decimal display units; persisted memory settings retain their binary units.
pub(crate) fn human_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.2} GB", bytes as f64 / 1_000_000_000.)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.)
    } else if bytes >= 1_000 {
        format!("{:.1} kB", bytes as f64 / 1_000.)
    } else {
        format!("{bytes} B")
    }
}

pub(crate) fn format_mib_as_mb(value: f64) -> String {
    format!("{:.2}", value * 1.048576)
}

pub(crate) fn parse_mb_as_mib(text: &str) -> Option<f64> {
    let value: f64 = text.trim().replace(',', ".").parse().ok()?;
    (value.is_finite() && value >= 0.).then(|| (value / 1.048576).round())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_display_and_editing_preserve_existing_quotas() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(999), "999 B");
        assert_eq!(human_bytes(1_000), "1.0 kB");
        assert_eq!(human_bytes(1_000_000), "1.0 MB");
        assert_eq!(human_bytes(1_000_000_000), "1.00 GB");
        assert_eq!(human_bytes(1 << 30), "1.07 GB");
        for mib in 0..=65536 {
            assert_eq!(
                parse_mb_as_mib(&format_mib_as_mb(mib as f64)),
                Some(mib as f64)
            );
        }
        assert_eq!(parse_mb_as_mib("1,05"), Some(1.));
        assert_eq!(parse_mb_as_mib("1000"), Some(954.));
        for invalid in ["NaN", "inf", "-1", "", "abc"] {
            assert_eq!(parse_mb_as_mib(invalid), None);
        }
    }
}
