//! How a length of time reads as text.

/// `seconds` rounded to whole seconds as "m:ss".
pub(crate) fn minutes_seconds(seconds: f64) -> String {
    let whole = seconds.round() as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}
