//! HTTP `Range` header parsing (RFC 9110 section 14), single byte-range only. Phones use it
//! to resume an interrupted download; anything fancier (multi-range) is ignored, which the RFC allows,
//! so the client simply gets the whole file.

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum RangeSpec {
    /// No usable Range header: send the whole file with 200.
    Full,
    /// Send `start..=end` (inclusive) with 206.
    Partial { start: u64, end: u64 },
    /// The range starts past the end of the file: 416.
    Unsatisfiable,
}

pub fn parse_range(header: Option<&str>, len: u64) -> RangeSpec {
    let Some(h) = header else {
        return RangeSpec::Full;
    };
    let h = h.trim();
    let Some(spec) = h.strip_prefix("bytes=") else {
        return RangeSpec::Full;
    };
    if spec.contains(',') || len == 0 {
        return RangeSpec::Full;
    }
    let Some((a, b)) = spec.split_once('-') else {
        return RangeSpec::Full;
    };
    let (a, b) = (a.trim(), b.trim());
    match (a.is_empty(), b.is_empty()) {
        (true, true) => RangeSpec::Full,
        // "-N": the last N bytes
        (true, false) => match b.parse::<u64>() {
            Ok(0) => RangeSpec::Unsatisfiable,
            Ok(n) => RangeSpec::Partial {
                start: len.saturating_sub(n),
                end: len - 1,
            },
            Err(_) => RangeSpec::Full,
        },
        // "N-" or "N-M"
        (false, _) => {
            let Ok(start) = a.parse::<u64>() else {
                return RangeSpec::Full;
            };
            if start >= len {
                return RangeSpec::Unsatisfiable;
            }
            if b.is_empty() {
                return RangeSpec::Partial {
                    start,
                    end: len - 1,
                };
            }
            match b.parse::<u64>() {
                Ok(end) if end < start => RangeSpec::Full, // invalid: ignore the header
                Ok(end) => RangeSpec::Partial {
                    start,
                    end: end.min(len - 1),
                },
                Err(_) => RangeSpec::Full,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use RangeSpec::*;

    #[test]
    fn no_header_means_full() {
        assert_eq!(parse_range(None, 100), Full);
    }

    #[test]
    fn open_ended_and_closed_ranges() {
        assert_eq!(
            parse_range(Some("bytes=0-"), 100),
            Partial { start: 0, end: 99 }
        );
        assert_eq!(
            parse_range(Some("bytes=10-19"), 100),
            Partial { start: 10, end: 19 }
        );
        assert_eq!(
            parse_range(Some("bytes=90-"), 100),
            Partial { start: 90, end: 99 }
        );
    }

    #[test]
    fn end_is_clamped_to_the_file() {
        assert_eq!(
            parse_range(Some("bytes=50-5000"), 100),
            Partial { start: 50, end: 99 }
        );
    }

    #[test]
    fn suffix_ranges() {
        assert_eq!(
            parse_range(Some("bytes=-10"), 100),
            Partial { start: 90, end: 99 }
        );
        assert_eq!(
            parse_range(Some("bytes=-500"), 100),
            Partial { start: 0, end: 99 }
        );
        assert_eq!(parse_range(Some("bytes=-0"), 100), Unsatisfiable);
    }

    #[test]
    fn start_past_end_is_unsatisfiable() {
        assert_eq!(parse_range(Some("bytes=100-"), 100), Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=500-600"), 100), Unsatisfiable);
    }

    #[test]
    fn garbage_and_multi_range_fall_back_to_full() {
        assert_eq!(parse_range(Some("bytes=abc"), 100), Full);
        assert_eq!(parse_range(Some("bytes=-"), 100), Full);
        assert_eq!(parse_range(Some("items=0-5"), 100), Full);
        assert_eq!(parse_range(Some("bytes=0-5,10-15"), 100), Full);
        assert_eq!(parse_range(Some("bytes=20-10"), 100), Full);
        assert_eq!(parse_range(Some("bytes=x-5"), 100), Full);
        assert_eq!(parse_range(Some("bytes=5-y"), 100), Full);
    }

    #[test]
    fn empty_file_ignores_ranges() {
        assert_eq!(parse_range(Some("bytes=0-"), 0), Full);
    }

    #[test]
    fn single_byte_range() {
        assert_eq!(
            parse_range(Some("bytes=0-0"), 100),
            Partial { start: 0, end: 0 }
        );
        assert_eq!(
            parse_range(Some("bytes=99-99"), 100),
            Partial { start: 99, end: 99 }
        );
    }
}
