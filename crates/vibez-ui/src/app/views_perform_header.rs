//! How many rows Perform's header takes across a workspace of a given
//! width.

const ONE_ROW_MIN_WIDTH: f32 = 1_120.0;
const TWO_ROWS_MIN_WIDTH: f32 = 680.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HeaderRows {
    One,
    Two,
    Three,
}

pub(super) fn header_rows(workspace_width: f32) -> HeaderRows {
    if workspace_width >= ONE_ROW_MIN_WIDTH {
        HeaderRows::One
    } else if workspace_width >= TWO_ROWS_MIN_WIDTH {
        HeaderRows::Two
    } else {
        HeaderRows::Three
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_adds_rows_before_controls_or_status_can_be_squashed() {
        assert_eq!(header_rows(ONE_ROW_MIN_WIDTH), HeaderRows::One);
        assert_eq!(header_rows(ONE_ROW_MIN_WIDTH - 1.0), HeaderRows::Two);
        assert_eq!(header_rows(TWO_ROWS_MIN_WIDTH), HeaderRows::Two);
        assert_eq!(header_rows(TWO_ROWS_MIN_WIDTH - 1.0), HeaderRows::Three);
    }
}
