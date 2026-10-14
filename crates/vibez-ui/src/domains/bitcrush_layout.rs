//! Fits Bitcrush's genuine waveform, bounded core controls and accessible detail pages.

#[cfg(test)]
mod tests {
    use super::{arrange, DetailCell, DetailPage, Region};
    use vibez_dsp::bitcrush::param;

    fn assert_inside(region: Region, parent: Region) {
        assert!(region.width > 0.0 && region.height > 0.0);
        assert!(region.x >= parent.x && region.y >= parent.y);
        assert!(region.x + region.width <= parent.x + parent.width + 0.001);
        assert!(region.y + region.height <= parent.y + parent.height + 0.001);
    }

    #[test]
    fn compact_minimum_keeps_all_six_pages_and_their_real_native_children_accessible() {
        use DetailCell::{
            ClockStatus, InputPeakLeft, InputPeakRight, InputRmsLeft, InputRmsRight,
            OutputPeakLeft, OutputPeakRight, OutputRmsLeft, OutputRmsRight, Parameter,
        };
        use DetailPage::{Clock, Color, Filter, Input, Main, Output};

        let cases: [(DetailPage, &[DetailCell]); 6] = [
            (
                Main,
                &[
                    Parameter(param::BITS),
                    Parameter(param::SR_REDUCE),
                    Parameter(param::MIX),
                ],
            ),
            (
                Color,
                &[
                    Parameter(param::DRIVE_DB),
                    Parameter(param::ROUNDING),
                    Parameter(param::DITHER),
                ],
            ),
            (
                Clock,
                &[
                    Parameter(param::SR_REDUCE),
                    Parameter(param::JITTER),
                    ClockStatus,
                ],
            ),
            (
                Filter,
                &[
                    Parameter(param::POST_LP_ENABLED),
                    Parameter(param::POST_LP_HZ),
                ],
            ),
            (
                Input,
                &[InputPeakLeft, InputPeakRight, InputRmsLeft, InputRmsRight],
            ),
            (
                Output,
                &[OutputPeakLeft, OutputPeakRight, OutputRmsLeft, OutputRmsRight],
            ),
        ];
        assert_eq!(DetailPage::ALL, [Main, Color, Clock, Filter, Input, Output]);

        let bounds = Region {
            x: 0.0,
            y: 0.0,
            width: 430.0,
            height: 64.0,
        };
        let native_field_height = 8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0;
        let four_readings_width = 4.0 * 68.0 + 3.0 * 6.0;
        for (page, required) in cases {
            let layout = arrange(bounds.width, bounds.height, true, page);
            assert_inside(layout.presets, bounds);
            assert_inside(layout.disclosure, bounds);
            assert!(layout.presets.width >= 140.0 + 72.0);
            assert!(layout.presets.height >= 26.0);
            assert!(layout.disclosure.width >= 60.0);
            assert!(layout.disclosure.height >= 9.0 * 1.3 + 6.0);
            assert!(
                layout.presets.x + layout.presets.width + 8.0 <= layout.disclosure.x,
                "preset actions overlap the disclosure on {page:?}"
            );
            assert!(layout.scope.is_none());

            let details = layout.details.expect("every minimum page needs a region");
            assert_eq!(details.page, page);
            assert_inside(details.bounds, bounds);
            assert_inside(details.selector, details.bounds);
            assert_inside(details.content, details.bounds);
            assert!(details.bounds.y >= layout.presets.y + layout.presets.height);
            assert!(details.selector.width >= 88.0);
            assert!(details.selector.height >= native_field_height);
            assert!(details.content.height >= native_field_height);
            assert!(details.content.width >= four_readings_width);
            assert!(
                details.selector.x + details.selector.width + 8.0 <= details.content.x,
                "page selection overlaps native fields on {page:?}"
            );
            assert_eq!(details.cells.len(), required.len());
            for cell in required {
                assert!(
                    details.cells.contains(cell),
                    "{page:?} lost an accessible child: {cell:?}"
                );
            }
        }
    }
}
