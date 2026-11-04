//! A Clip drag's preview on one lane: the outline where the Clip began, the
//! guide at the line it snapped to, the readout beside the pointer and the
//! shadow that lifts the dragged Clip.

#[cfg(test)]
mod tests {
    use super::readout_box;
    use iced::Point;

    #[test]
    fn the_readout_sits_up_and_right_of_the_pointer_on_its_own_lane() {
        let pill = readout_box(Point::new(200.0, 150.0), 40.0, 100.0, 800.0, 70.0).unwrap();

        assert_eq!((pill.x, pill.y), (214.0, 20.0));
        assert_eq!(
            readout_box(Point::new(200.0, 90.0), 40.0, 100.0, 800.0, 70.0),
            None
        );
    }

    #[test]
    fn the_readout_never_leaves_its_lane() {
        let at_right = readout_box(Point::new(790.0, 105.0), 40.0, 100.0, 800.0, 70.0).unwrap();

        assert!(at_right.x + at_right.width <= 800.0);
        assert!(at_right.y >= 0.0);
    }
}
