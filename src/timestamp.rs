use chrono::Local;

/// Strftime pattern for snapshot timestamps. The format is chosen so that
/// lexicographic order equals chronological order, which every "find the
/// last …" lookup in [`crate::storage`] relies on.
const FORMAT: &str = "%Y-%m-%d-%H%M";

/// Validation shape for a formatted timestamp: `0` must be an ASCII digit,
/// anything else must match literally.
const SHAPE: &str = "0000-00-00-0000";

pub fn now() -> String {
    Local::now().format(FORMAT).to_string()
}

pub fn is_valid(text: &str) -> bool {
    text.len() == SHAPE.len()
        && text
            .bytes()
            .zip(SHAPE.bytes())
            .all(|(actual, shape)| match shape {
                b'0' => actual.is_ascii_digit(),
                literal => actual == literal,
            })
}

/// The timestamp an archive or snar file name starts with, if any.
pub fn leading(file_name: &str) -> Option<&str> {
    let head = file_name.get(..SHAPE.len())?;
    is_valid(head).then_some(head)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_timestamp() {
        assert!(is_valid("2026-08-12-1530"));
    }

    #[test]
    fn rejects_malformed_timestamps() {
        for bad in [
            "",
            "2026-08-12",
            "2026-08-12-15300",
            "2026_08_12_1530",
            "yyyy-mm-dd-hhmm",
        ] {
            assert!(!is_valid(bad), "accepted {bad:?}");
        }
    }

    #[test]
    fn extracts_leading_timestamp_from_incremental_snar_name() {
        assert_eq!(
            leading("2026-01-01-1800.2025-12-31-0200.snar"),
            Some("2026-01-01-1800")
        );
        assert_eq!(leading("notes.txt"), None);
    }

    #[test]
    fn now_produces_a_valid_timestamp() {
        assert!(is_valid(&now()));
    }
}
