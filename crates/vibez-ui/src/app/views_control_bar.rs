//! The control bar across the top of the window, in one row when the window
//! is wide enough and two when it is not.

#[cfg(test)]
mod tests {
    use super::{control_bar_rows, workspace_segments, ControlBarRows};
    use crate::state::Workspace;

    #[test]
    fn the_workspace_switch_marks_only_the_current_workspace() {
        for current in [Workspace::Perform, Workspace::Arrange, Workspace::Mix] {
            let selected: Vec<Workspace> = workspace_segments(current)
                .iter()
                .filter(|segment| segment.selected)
                .map(|segment| segment.workspace)
                .collect();
            assert_eq!(selected, vec![current]);
        }
        assert_eq!(
            workspace_segments(Workspace::Mix).map(|segment| segment.label),
            ["Perform", "Arrange", "Mix"]
        );
    }

    #[test]
    fn the_control_bar_is_one_row_when_it_fits_and_two_when_it_does_not() {
        assert_eq!(control_bar_rows(1440.0), ControlBarRows::One);
        assert_eq!(control_bar_rows(960.0), ControlBarRows::Two);
    }
}
