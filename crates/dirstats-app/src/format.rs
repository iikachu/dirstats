// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors

//! Human-readable formatting shared by front ends.

/// Binary-prefixed size, e.g. `12.3 MiB`.
#[must_use]
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 { format!("{value:.0} {}", UNITS[unit]) } else { format!("{value:.1} {}", UNITS[unit]) }
}

/// Percentage of `part` in `whole`, 0 when `whole` is 0.
#[must_use]
pub fn percent(part: u64, whole: u64) -> f64 {
    if whole == 0 { 0.0 } else { part as f64 * 100.0 / whole as f64 }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sizes() {
        assert_eq!(super::size(0), "0 B");
        assert_eq!(super::size(1023), "1023 B");
        assert_eq!(super::size(1536), "1.5 KiB");
        assert_eq!(super::size(150 << 20), "150 MiB");
    }
}
