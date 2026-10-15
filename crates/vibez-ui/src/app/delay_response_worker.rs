//! Serializes bounded Delay reference work until its history-owning closure has exited.

use std::collections::VecDeque;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use iced::Task;
use vibez_core::{constants::MAX_EFFECTS_PER_TRACK, delay_state::DelayState, id::{EffectId, TrackId}};
use crate::domains::delay_response::{DelayResponse, render_reference};
use super::{tasks::run_off_ui_thread, tracked_request::TrackedRequest};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Owner {
    pub track_id: TrackId,
    pub effect_id: EffectId,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ReferenceKey {
    sample_rate: u32,
    left_samples: u64,
    right_samples: u64,
    feedback: u32,
    routing: u32,
    high_pass: Option<u64>,
    low_pass: Option<u64>,
}

impl ReferenceKey {
    pub fn of(state: &DelayState) -> Self {
        Self {
            sample_rate: state.sample_rate,
            left_samples: state.lines[0].destination_samples.to_bits(),
            right_samples: state.lines[1].destination_samples.to_bits(),
            feedback: state.feedback.to_bits(),
            routing: state.routing.to_bits(),
            high_pass: (state.filters[0].enabled != 0.0).then(|| state.filters[0].cutoff_hz.to_bits()),
            low_pass: (state.filters[1].enabled != 0.0).then(|| state.filters[1].cutoff_hz.to_bits()),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Request {
    pub owner: Owner,
    pub key: ReferenceKey,
    state: DelayState,
}

impl Request {
    pub fn new(owner: Owner, state: DelayState) -> Self {
        Self { owner, key: ReferenceKey::of(&state), state }
    }
}

pub(super) type ResponseResult = Result<Option<Arc<DelayResponse>>, String>;
pub(super) type Completion = (u64, ResponseResult);

#[derive(Default)]
struct Control {
    cancel: AtomicBool,
    exited: AtomicBool,
}

struct ExitGuard(Arc<Control>);

impl Drop for ExitGuard {
    fn drop(&mut self) {
        self.0.exited.store(true, Ordering::Release);
    }
}

pub(super) struct Work {
    pub token: u64,
    request: Request,
    exit: ExitGuard,
}

impl Work {
    pub async fn run(self) -> Completion {
        let Self { token, request, exit } = self;
        let result = run_off_ui_thread("Delay reference", move || {
            // The guard moves through the unpolled future into the blocking closure.
            // render_reference destroys its private history before this guard acknowledges exit.
            let _exit = exit;
            render_reference(&request.state, &_exit.0.cancel).map(Arc::new)
        }).await;
        (token, result)
    }
}

struct Active {
    token: u64,
    request: Request,
    control: Arc<Control>,
}

#[derive(Default)]
pub(super) struct DelayResponseWorker {
    tracked: TrackedRequest,
    visible: Vec<Owner>,
    pending: VecDeque<Request>,
    active: Option<Active>,
}

impl DelayResponseWorker {
    pub fn reconcile(&mut self, visible: &[Owner]) -> bool {
        if visible.len() > MAX_EFFECTS_PER_TRACK { return false; }
        self.visible.clear();
        for &owner in visible {
            if !self.visible.contains(&owner) { self.visible.push(owner); }
        }
        self.pending.retain(|request| self.visible.contains(&request.owner));
        if self.active.as_ref().is_some_and(|active| !self.visible.contains(&active.request.owner)) {
            self.cancel_active();
        }
        true
    }

    pub fn offer(&mut self, request: Request) -> bool {
        if !self.visible.contains(&request.owner) { return false; }
        if let Some(active) = self.active.as_ref().filter(|active| active.request.owner == request.owner) {
            if self.tracked.is_current(active.token) && active.request.key == request.key {
                return false;
            }
            self.cancel_active();
        }
        if let Some(pending) = self.pending.iter_mut().find(|pending| pending.owner == request.owner) {
            *pending = request;
        } else {
            self.pending.push_back(request);
        }
        true
    }

    pub fn cancel(&mut self, owner: Owner) {
        self.pending.retain(|request| request.owner != owner);
        if self.active.as_ref().is_some_and(|active| active.request.owner == owner) {
            self.cancel_active();
        }
    }

    fn cancel_active(&mut self) {
        if let Some(active) = &self.active {
            active.control.cancel.store(true, Ordering::Release);
            self.tracked.cancel();
        }
    }

    pub fn admit(&mut self) -> Option<Work> {
        if let Some(active) = &self.active {
            // Task abort does not stop an already spawned blocking closure. A normal
            // completion also retains the slot until the router consumes its result.
            if self.tracked.is_current(active.token) || !active.control.exited.load(Ordering::Acquire) {
                return None;
            }
            self.active = None;
        }
        let request = self.pending.pop_front()?;
        let token = self.tracked.begin();
        let control = Arc::new(Control::default());
        self.active = Some(Active { token, request, control: Arc::clone(&control) });
        Some(Work { token, request, exit: ExitGuard(control) })
    }

    pub fn attach<Message: 'static>(&mut self, token: u64, task: Task<Message>) -> Task<Message> {
        if self.tracked.is_current(token) { self.tracked.attach(task) } else { Task::none() }
    }

    pub fn complete(&mut self, token: u64, result: ResponseResult) -> Option<(Request, ResponseResult)> {
        let active = self.active.as_ref()?;
        if active.token != token || !active.control.exited.load(Ordering::Acquire)
            || !self.tracked.finish(token) { return None; }
        let active = self.active.take()?;
        Some((active.request, result))
    }
}

impl Drop for DelayResponseWorker {
    fn drop(&mut self) {
        self.cancel_active();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_dsp::{delay::{DelayEffect, param}, effect::AudioEffect};

    fn owner() -> Owner {
        Owner { track_id: TrackId::new(), effect_id: EffectId::new(), generation: 1 }
    }

    fn state() -> DelayState {
        let mut effect = DelayEffect::new(1000.0);
        assert!(effect.set_param(param::TIME_MS, 3.0));
        let mut sample = [0.0; 2];
        effect.process(&mut sample, 2);
        effect.delay_snapshot().unwrap()
    }

    fn changed(owner: Owner, feedback: f32) -> Request {
        let mut state = state();
        state.feedback = feedback;
        state.targets[param::FEEDBACK] = feedback;
        Request::new(owner, state)
    }

    #[test]
    fn latest_per_owner_keeps_fifo_position_and_capacity_matches_the_visible_chain() {
        let owners: Vec<_> = (0..MAX_EFFECTS_PER_TRACK).map(|_| owner()).collect();
        let mut worker = DelayResponseWorker::default();
        assert!(worker.reconcile(&owners));
        for &owner in &owners { assert!(worker.offer(changed(owner, 0.2))); }
        assert!(worker.offer(changed(owners[3], 0.7)));
        assert_eq!(worker.pending.len(), MAX_EFFECTS_PER_TRACK);
        let mut too_many = owners.clone();
        too_many.push(owner());
        assert!(!worker.reconcile(&too_many));
        assert_eq!(worker.pending.len(), MAX_EFFECTS_PER_TRACK);
        assert!(!worker.offer(changed(owner(), 0.3)));
        for (index, owner) in owners.iter().enumerate() {
            let work = worker.admit().unwrap();
            let token = work.token;
            assert_eq!(work.request.owner, *owner);
            assert_eq!(work.request.state.feedback, if index == 3 { 0.7 } else { 0.2 });
            drop(work);
            assert!(worker.admit().is_none());
            assert!(worker.complete(token, Ok(None)).is_some());
        }
        assert!(worker.admit().is_none());
    }

    #[test]
    fn aborted_unpolled_task_stays_busy_until_its_guard_drops_then_other_owner_runs_first() {
        let (a, b) = (owner(), owner());
        let mut worker = DelayResponseWorker::default();
        assert!(worker.reconcile(&[a, b]));
        worker.offer(changed(a, 0.2));
        let work = worker.admit().unwrap();
        let old = work.token;
        let control = Arc::clone(&worker.active.as_ref().unwrap().control);
        let task = worker.attach(old, Task::perform(work.run(), std::convert::identity));
        worker.offer(changed(b, 0.3));
        worker.offer(changed(a, 0.7));
        assert!(!worker.tracked.is_current(old));
        assert!(control.cancel.load(Ordering::Acquire));
        assert!(!control.exited.load(Ordering::Acquire));
        assert!(worker.admit().is_none());
        drop(task);
        assert!(control.exited.load(Ordering::Acquire));
        let next = worker.admit().unwrap();
        assert_eq!(next.request.owner, b);
        assert!(worker.complete(old, Ok(None)).is_none());
        let next_token = next.token;
        drop(next);
        worker.complete(next_token, Ok(None)).unwrap();
        assert_eq!(worker.admit().unwrap().request.state.feedback, 0.7);
    }

    #[test]
    fn canceling_held_work_and_changing_subscription_cannot_overlap_history_jobs() {
        let a = owner();
        let replacement = Owner { generation: 2, ..a };
        let mut worker = DelayResponseWorker::default();
        worker.reconcile(&[a]);
        worker.offer(changed(a, 0.2));
        let held = worker.admit().unwrap();
        worker.cancel(a);
        worker.reconcile(&[replacement]);
        worker.offer(changed(replacement, 0.7));
        assert!(worker.admit().is_none());
        drop(held);
        assert_eq!(worker.admit().unwrap().request.owner, replacement);
    }

    #[tokio::test]
    async fn actual_render_exit_waits_for_consumption_and_rejects_previous_tokens() {
        let a = owner();
        let mut worker = DelayResponseWorker::default();
        worker.reconcile(&[a]);
        worker.offer(changed(a, 0.5));
        let work = worker.admit().unwrap();
        let control = Arc::clone(&worker.active.as_ref().unwrap().control);
        let (token, result) = work.run().await;
        assert!(control.exited.load(Ordering::Acquire));
        assert!(worker.admit().is_none());
        let (request, result) = worker.complete(token, result).unwrap();
        assert_eq!(request.owner, a);
        let response = result.unwrap().unwrap();
        assert_eq!(response.sample_rate, 1000);
        assert_eq!(response.frames, 13);
        assert!(response.bins.iter().any(|bin| bin.left == 1.0 && bin.left_frame == 3));
        worker.offer(changed(a, 0.7));
        let next = worker.admit().unwrap();
        assert_ne!(next.token, token);
        assert!(worker.complete(token, Ok(Some(response))).is_none());
        assert!(worker.tracked.is_current(next.token));
    }

    #[test]
    fn worker_shutdown_requests_cancellation_without_releasing_a_held_job() {
        let a = owner();
        let mut worker = DelayResponseWorker::default();
        worker.reconcile(&[a]);
        worker.offer(changed(a, 0.2));
        let held = worker.admit().unwrap();
        let control = Arc::clone(&worker.active.as_ref().unwrap().control);
        drop(worker);
        assert!(control.cancel.load(Ordering::Acquire));
        assert!(!control.exited.load(Ordering::Acquire));
        drop(held);
        assert!(control.exited.load(Ordering::Acquire));
    }

    #[test]
    fn reference_identity_ignores_wet_mix_duck_and_inactive_controls() {
        let original = state();
        let mut irrelevant = original;
        irrelevant.mix = 0.1;
        irrelevant.duck_attenuation_db = 12.0;
        irrelevant.targets[param::MIX] = 0.1;
        irrelevant.targets[param::DUCK] = 0.8;
        irrelevant.targets[param::RIGHT_TIME_MS] = 17.0;
        irrelevant.targets[param::RIGHT_DIVISION] = 2.0;
        irrelevant.targets[param::LEFT_DIVISION] = 1.0;
        irrelevant.targets[param::FEEDBACK_HP_HZ] = 400.0;
        irrelevant.filters[0].cutoff_hz = 400.0;
        assert_eq!(ReferenceKey::of(&original), ReferenceKey::of(&irrelevant));
        irrelevant.filters[0].enabled = 1.0;
        assert_ne!(ReferenceKey::of(&original), ReferenceKey::of(&irrelevant));
        let mut rate = original;
        rate.sample_rate = 1001;
        assert_ne!(ReferenceKey::of(&original), ReferenceKey::of(&rate));
    }
}
