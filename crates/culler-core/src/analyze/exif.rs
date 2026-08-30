//! EXIF extraction via kamadak-exif: capture time, camera, orientation.

use std::path::Path;

/// EXIF-derived facts; every field is best-effort.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExifFacts {
    /// DateTimeOriginal as unix seconds (no timezone in EXIF; treated as UTC).
    pub captured_at: Option<i64>,
    /// Make and model, joined without repeating the make.
    pub camera: Option<String>,
    pub orientation: Option<u16>,
}

/// Reads EXIF from any container kamadak-exif understands (JPEG, TIFF, HEIF,
/// PNG, WebP). Missing/corrupt EXIF yields default facts, not an error.
pub fn read_exif(path: &Path) -> ExifFacts {
    use exif::{In, Tag, Value};

    let Ok(file) = std::fs::File::open(path) else {
        return ExifFacts::default();
    };
    let mut reader = std::io::BufReader::new(file);
    let Ok(data) = exif::Reader::new().read_from_container(&mut reader) else {
        return ExifFacts::default();
    };

    let ascii = |tag: Tag| {
        data.get_field(tag, In::PRIMARY)
            .and_then(|field| match &field.value {
                Value::Ascii(chunks) => chunks.first().map(|bytes| {
                    String::from_utf8_lossy(bytes)
                        .trim_matches(char::from(0))
                        .trim()
                        .to_owned()
                }),
                _ => None,
            })
            .filter(|s| !s.is_empty())
    };

    let captured_at = ascii(Tag::DateTimeOriginal)
        .or_else(|| ascii(Tag::DateTime))
        .and_then(|s| parse_exif_datetime(&s));
    let camera = match (ascii(Tag::Make), ascii(Tag::Model)) {
        // Many models already repeat the make ("Canon EOS R5").
        (Some(make), Some(model)) if model.starts_with(&make) => Some(model),
        (Some(make), Some(model)) => Some(format!("{make} {model}")),
        (make, model) => model.or(make),
    };
    let orientation = data
        .get_field(Tag::Orientation, In::PRIMARY)
        .and_then(|field| field.value.get_uint(0))
        .and_then(|v| u16::try_from(v).ok());

    ExifFacts {
        captured_at,
        camera,
        orientation,
    }
}

/// Parses EXIF "YYYY:MM:DD HH:MM:SS" into unix seconds.
fn parse_exif_datetime(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 19
        || b[4] != b':'
        || b[7] != b':'
        || b[10] != b' '
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |range: std::ops::Range<usize>| s.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None; // also rejects the "0000:00:00" unset placeholder
    }
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86400 + hour * 3600 + minute * 60 + second)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`), so no chrono dependency is needed.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

#[cfg(test)]
mod tests {
    use super::parse_exif_datetime;

    #[test]
    fn parses_exif_datetime_to_unix_seconds() {
        // date -u -j -f "%Y-%m-%d %T" "2019-07-15 14:30:05" +%s
        assert_eq!(parse_exif_datetime("2019:07:15 14:30:05"), Some(1563201005));
        assert_eq!(parse_exif_datetime("1970:01:01 00:00:00"), Some(0));
        // Leap day, and a pre-1970 date.
        assert_eq!(parse_exif_datetime("2024:02:29 12:00:00"), Some(1709208000));
        assert_eq!(parse_exif_datetime("1969:12:31 23:59:59"), Some(-1));
    }

    #[test]
    fn rejects_garbage_datetimes() {
        assert_eq!(parse_exif_datetime(""), None);
        assert_eq!(parse_exif_datetime("not a date"), None);
        assert_eq!(parse_exif_datetime("2019:13:40 99:99:99"), None);
        // A common camera placeholder for "unset".
        assert_eq!(parse_exif_datetime("0000:00:00 00:00:00"), None);
    }
}
