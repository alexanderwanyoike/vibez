//! Measured regions for the native Filter body and its modulation disclosure.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advanced_retains_response_output_and_primary_controls_when_the_panel_has_room() {
        for (width, height) in [430.0, 660.0, 900.0].into_iter()
            .flat_map(|width| [304.0, 318.0].map(|height| (width, height))) {
            let layout = arrange(width, height, true);
            let response = layout.response.expect("Advanced must retain the response");
            let meter = layout.meter.expect("Advanced must retain output metering");
            let modulation = layout.modulation.expect("Advanced must expose modulation");
            assert!(response.width >= 200.0, "response too narrow at {width}");
            assert!(response.height >= 80.0, "response too short at {width}");
            assert!(meter.height >= 80.0, "meter too short at {width}");
            assert!(layout.primary.height >= 60.0, "core controls lost at {width}");
            for region in [response, meter, layout.primary, modulation] {
                assert!(region.x >= 0.0 && region.y >= 0.0);
                assert!(region.x + region.width <= width + 0.01);
                assert!(region.y + region.height <= height + 0.01);
            }
            for (left, right) in [(response, modulation), (meter, modulation),
                (layout.primary, modulation)] {
                let overlap_x = left.x < right.x + right.width && right.x < left.x + left.width;
                let overlap_y = left.y < right.y + right.height && right.y < left.y + left.height;
                assert!(!(overlap_x && overlap_y), "disclosure overlaps content at {width}");
            }
        }
    }
}
