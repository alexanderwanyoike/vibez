//! Labelled cells in the header column beside the Arrange ruler and minimap.

/// Song length beside the minimap, in bars and minutes.
pub fn song_length_label(seconds: f64, bpm: f64) -> String {
    let _ = (seconds, bpm);
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_song_length_reads_in_whole_bars_and_minutes() {
        assert_eq!(song_length_label(140.0, 120.0), "70 bars, 2:20");
        assert_eq!(song_length_label(61.0, 120.0), "31 bars, 1:01");
        assert_eq!(song_length_label(2.0, 120.0), "1 bar, 0:02");
    }

    #[test]
    fn an_empty_song_has_no_length() {
        assert_eq!(song_length_label(0.0, 120.0), "Empty");
    }
}
