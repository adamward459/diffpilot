pub fn grouped(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

pub fn compact(value: u32) -> String {
    let number = f64::from(value);
    let (scaled, suffix) = if number >= 999_500.0 {
        (number / 1e6, "M")
    } else if number >= 1000.0 {
        (number / 1e3, "K")
    } else {
        return value.to_string();
    };
    if scaled < 9.95 {
        let text = format!("{scaled:.1}");
        format!("{}{suffix}", text.strip_suffix(".0").unwrap_or(&text))
    } else {
        format!("{scaled:.0}{suffix}")
    }
}

pub fn file_count(count: usize) -> String {
    let noun = if count == 1 { "file" } else { "files" };
    format!("{} {noun}", grouped(count))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(2000), "2,000");
        assert_eq!(grouped(1234567), "1,234,567");
    }

    #[test]
    fn compacts_like_the_swift_summary() {
        assert_eq!(compact(0), "0");
        assert_eq!(compact(999), "999");
        assert_eq!(compact(1000), "1K");
        assert_eq!(compact(1234), "1.2K");
        assert_eq!(compact(37_412), "37K");
        assert_eq!(compact(11_000), "11K");
        assert_eq!(compact(1_500_000), "1.5M");
        assert_eq!(compact(9_999), "10K");
        assert_eq!(compact(999_900), "1M");
    }

    #[test]
    fn counts_files() {
        assert_eq!(file_count(1), "1 file");
        assert_eq!(file_count(2000), "2,000 files");
    }
}
