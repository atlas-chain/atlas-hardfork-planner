pub fn parse_quantity(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16).ok()
    } else {
        trimmed.parse::<u64>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decimal_values() {
        assert_eq!(parse_quantity("440000000"), Some(440_000_000));
        assert_eq!(parse_quantity("0"), Some(0));
    }

    #[test]
    fn parses_hex_values() {
        assert_eq!(parse_quantity("0x1a3b"), Some(0x1a3b));
        assert_eq!(parse_quantity("0X1A3B"), Some(0x1a3b));
        assert_eq!(parse_quantity("0x0"), Some(0));
    }

    #[test]
    fn rejects_invalid_values() {
        assert_eq!(parse_quantity(""), None);
        assert_eq!(parse_quantity("   "), None);
        assert_eq!(parse_quantity("0x"), None);
        assert_eq!(parse_quantity("0xnope"), None);
        assert_eq!(parse_quantity("-1"), None);
        assert_eq!(parse_quantity("1_000"), None);
        assert_eq!(parse_quantity("abc"), None);
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(parse_quantity("  7  "), Some(7));
    }
}
