use super::*;

#[test]
fn supports_bars_subdivisions_and_triplets() {
    assert_eq!(SnapGrid::EIGHT_BARS.beat_size(), 32.0);
    assert_eq!(SnapGrid::FOUR_BARS.beat_size(), 16.0);
    assert_eq!(SnapGrid::BAR.beat_size(), 4.0);
    assert_eq!(SnapGrid::EIGHTH.beat_size(), 0.5);
    assert!((SnapGrid::EIGHTH.triplet().beat_size() - 1.0 / 3.0).abs() < 1e-9);
    assert_eq!(SnapGrid::EIGHTH.triplet().label(), "1/8T");
}

#[test]
fn narrower_and_wider_preserve_triplet_mode() {
    assert_eq!(SnapGrid::EIGHT_BARS.narrower(), SnapGrid::FOUR_BARS);
    assert_eq!(SnapGrid::FOUR_BARS.wider(), SnapGrid::EIGHT_BARS);
    assert_eq!(SnapGrid::BAR.narrower(), SnapGrid::QUARTER);
    assert_eq!(SnapGrid::QUARTER.wider(), SnapGrid::BAR);
    assert_eq!(
        SnapGrid::EIGHTH.triplet().narrower(),
        SnapGrid::SIXTEENTH.triplet()
    );
    assert_eq!(SnapGrid::THIRTY_SECOND.narrower(), SnapGrid::THIRTY_SECOND);
    assert_eq!(SnapGrid::EIGHT_BARS.wider(), SnapGrid::EIGHT_BARS);
}

#[test]
fn adaptive_grid_gets_narrower_as_pixels_per_beat_increase() {
    assert_eq!(SnapGrid::adaptive(0.25, 0, false), SnapGrid::EIGHT_BARS);
    assert_eq!(SnapGrid::adaptive(5.0, 0, false), SnapGrid::BAR);
    assert_eq!(SnapGrid::adaptive(20.0, 0, false), SnapGrid::QUARTER);
    assert_eq!(SnapGrid::adaptive(80.0, 0, false), SnapGrid::SIXTEENTH);
    assert_eq!(
        SnapGrid::adaptive(80.0, 0, true),
        SnapGrid::SIXTEENTH.triplet()
    );
}

#[test]
fn grid_config_resolves_adaptive_density_and_can_disable_snapping() {
    let fixed = GridConfig::new(SnapGrid::EIGHTH, true, false, 0);
    assert_eq!(fixed.effective_grid(80.0), SnapGrid::EIGHTH);
    assert_eq!(fixed.snap_beat(0.31, 80.0), 0.5);

    let adaptive = GridConfig::new(SnapGrid::EIGHTH.triplet(), true, true, 0);
    assert_eq!(adaptive.effective_grid(80.0), SnapGrid::SIXTEENTH.triplet());

    let free = GridConfig::new(SnapGrid::SIXTEENTH, false, false, 0);
    assert_eq!(free.snap_beat(0.31, 80.0), 0.31);
}

#[test]
fn choosing_auto_in_the_snap_list_returns_a_fixed_grid_to_zoom_adaptive() {
    let mut grid = GridConfig::default();
    grid.choose(GridChoice::Fixed(SnapGrid::EIGHTH));
    assert_eq!(grid.effective_grid(1.375), SnapGrid::EIGHTH);

    let auto = grid.choices(1.375)[0];
    assert_eq!(auto.to_string(), "Auto 4 Bars");
    grid.choose(auto);
    assert_eq!(grid.effective_grid(1.375), SnapGrid::FOUR_BARS);
    assert_eq!(grid.choice(1.375), auto);
}

#[test]
fn the_snap_list_shows_the_step_the_grid_snaps_to() {
    let grid = GridConfig::default();
    assert_eq!(grid.choice(80.0).to_string(), "Auto 1/16");

    let mut fixed = grid;
    fixed.choose(GridChoice::Fixed(SnapGrid::SIXTEENTH.triplet()));
    assert_eq!(fixed.choice(80.0).to_string(), "1/16T");
    assert!(fixed.choices(80.0).contains(&fixed.choice(80.0)));
}

fn trimmed_note_clip(loop_enabled: bool) -> UiNoteClip {
    UiNoteClip {
        name: "Trimmed".into(),
        start_marker_beats: 6.0,
        loop_enabled,
        loop_end_beats: 8.0,
        ..UiNoteClip::test_clip(6.0, 2.0)
    }
}

#[test]
fn a_clip_editor_shows_a_trimmed_note_clip_up_to_where_it_stops_playing() {
    assert_eq!(trimmed_note_clip(false).editor_beats(), 8.0);
}

#[test]
fn a_clip_editor_shows_a_looped_note_clip_over_its_length() {
    assert_eq!(trimmed_note_clip(true).editor_beats(), 2.0);
}
