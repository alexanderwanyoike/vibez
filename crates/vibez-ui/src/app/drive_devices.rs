//! Native Drive cards on the visible device chain and their panel state.

use crate::domains::drive::DriveMsg;

use super::*;

impl App {
    pub(super) fn update_drive(&mut self, message: DriveMsg) -> Task<Message> {
        self.drive_panels.entry(message.effect_id()).update(message);
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::devices::DevicesMsg;
    use crate::state::{DetailPanelTab, ProjectTrack};
    use vibez_core::effect::EffectType;
    use vibez_core::id::{EffectId, TrackId};
    use vibez_engine::metering::MeterValues;

    fn drive_app() -> (App, TrackId, EffectId) {
        let mut app = super::super::test_support::app();
        let track = ProjectTrack::new(TrackId::new(), "Bass".into(), 1);
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            id,
            EffectType::Drive,
        )));
        let effect = app.state.find_track(id).unwrap().effects[0].id;
        app.state.arrangement.selected_track = Some(id);
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        app.poll_device_meters();
        (app, id, effect)
    }

    #[test]
    fn a_visible_drive_receives_its_levels() {
        let (mut app, track, effect) = drive_app();
        let generation = app.device_meters.generation;
        let level = MeterValues {
            peak_l: 0.5,
            peak_r: 0.4,
            rms_l: 0.3,
            rms_r: 0.2,
        };
        app.accept_device_meter(track, effect, generation.wrapping_sub(1), level, level);
        assert_eq!(app.drive_panels.panel(effect).input, MeterValues::SILENT);
        app.accept_device_meter(track, effect, generation, level, MeterValues::SILENT);
        assert_eq!(app.drive_panels.panel(effect).input, level);
    }

    #[test]
    fn a_new_drive_carries_its_latency_into_compensation() {
        let (app, track, effect) = drive_app();
        let effect = app
            .state
            .find_track(track)
            .unwrap()
            .effects
            .iter()
            .find(|candidate| candidate.id == effect)
            .unwrap();
        assert_eq!(
            effect.latency_samples,
            Some(vibez_dsp::drive::LATENCY_SAMPLES)
        );
    }

    #[tokio::test]
    async fn a_loaded_drive_carries_its_latency_into_compensation() {
        use super::super::{load_project_async, save_project_async};
        use vibez_core::effect::EffectInfo;
        use vibez_core::track::TrackInfo;
        let directory = tempfile::Builder::new()
            .prefix(".drive-project-test-")
            .tempdir_in(std::env::current_dir().unwrap())
            .unwrap();
        let mut project = vibez_project::Project::default();
        let mut track = TrackInfo::new("Old Drive");
        let track_id = track.id;
        let effect_id = EffectId::new();
        track.effects.push(EffectInfo {
            id: effect_id,
            effect_type: EffectType::Drive,
            bypass: false,
            params: vec![0.5, 0.5, 0.7, 1.0],
            plugin: None,
            sidechains: vec![],
            inactive_sidechains: vec![],
        });
        project.tracks.push(track);
        let path = directory.path().join("old-drive.vibez");
        save_project_async(path.clone(), None, project)
            .await
            .unwrap();
        let loaded = load_project_async(path, None).await.unwrap();
        let mut app = super::super::test_support::app();
        app.rebuild_from_loaded_project(loaded);
        let effect = app
            .state
            .find_track(track_id)
            .unwrap()
            .effects
            .iter()
            .find(|effect| effect.id == effect_id)
            .unwrap();
        assert_eq!(
            effect.latency_samples,
            Some(vibez_dsp::drive::LATENCY_SAMPLES)
        );
    }

    #[test]
    fn a_removed_drive_forgets_its_panel() {
        let (mut app, track, effect) = drive_app();
        let _ = app.update(Message::Drive(DriveMsg::ToggleAdvanced(effect)));
        let _ = app.update(Message::Devices(DevicesMsg::RemoveEffect(track, effect)));
        app.poll_device_meters();
        assert!(!app.drive_panels.contains(effect));
    }
}
