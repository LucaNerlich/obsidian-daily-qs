//! Subset of moment.js date tokens used by Obsidian daily notes.

use chrono::{Datelike, NaiveDate};

/// Format `date` with a moment-style pattern.
///
/// Supported tokens (longest match first): `YYYY`, `YY`, `MMMM`, `MMM`, `MM`,
/// `M`, `dddd`, `ddd`, `DD`, `D`. Literal text is preserved.
pub fn format_moment(pattern: &str, date: NaiveDate) -> Result<String, String> {
    if pattern.is_empty() {
        return Err("format is empty".into());
    }
    let mut out = String::with_capacity(pattern.len() + 8);
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some((token, len)) = match_token(&pattern[i..]) {
            out.push_str(&render_token(token, date)?);
            i += len;
            continue;
        }
        out.push(pattern[i..].chars().next().unwrap());
        i += pattern[i..].chars().next().unwrap().len_utf8();
    }
    Ok(out)
}

fn match_token(rest: &str) -> Option<(&'static str, usize)> {
    const TOKENS: &[&str] = &[
        "YYYY", "MMMM", "dddd", "ddd", "MMM", "YY", "MM", "DD", "M", "D",
    ];
    for token in TOKENS {
        if rest.starts_with(token) {
            return Some((token, token.len()));
        }
    }
    None
}

fn render_token(token: &str, date: NaiveDate) -> Result<String, String> {
    Ok(match token {
        "YYYY" => format!("{:04}", date.year()),
        "YY" => format!("{:02}", date.year() % 100),
        "MMMM" => month_name(date.month())?.to_string(),
        "MMM" => month_name(date.month())?[..3].to_string(),
        "MM" => format!("{:02}", date.month()),
        "M" => date.month().to_string(),
        "DD" => format!("{:02}", date.day()),
        "D" => date.day().to_string(),
        "dddd" => weekday_name(date)?.to_string(),
        "ddd" => weekday_name(date)?[..3].to_string(),
        other => return Err(format!("unsupported token {other}")),
    })
}

fn month_name(month: u32) -> Result<&'static str, String> {
    Ok(match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => return Err(format!("invalid month {month}")),
    })
}

fn weekday_name(date: NaiveDate) -> Result<&'static str, String> {
    Ok(match date.weekday().num_days_from_monday() {
        0 => "Monday",
        1 => "Tuesday",
        2 => "Wednesday",
        3 => "Thursday",
        4 => "Friday",
        5 => "Saturday",
        6 => "Sunday",
        _ => return Err("invalid weekday".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_default_iso() {
        let d = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
        assert_eq!(format_moment("YYYY-MM-DD", d).unwrap(), "2026-08-20");
    }

    #[test]
    fn formats_nested_folders() {
        let d = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        assert_eq!(
            format_moment("YYYY/MMMM/YYYY-MMM-DD", d).unwrap(),
            "2026/January/2026-Jan-05"
        );
    }

    #[test]
    fn formats_weekday() {
        let d = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap(); // Thursday
        assert_eq!(format_moment("dddd", d).unwrap(), "Thursday");
        assert_eq!(format_moment("ddd", d).unwrap(), "Thu");
    }
}
