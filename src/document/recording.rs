//! Recording what the simulation did, and playing it back.
//!
//! A run that produced something interesting is gone as soon as it advances.
//! This keeps the frames as they are displayed, so a user can stop, go back and
//! look at what happened rather than trying to catch it live.
//!
//! Frames are the snapshots the worker already publishes, held by handle. That
//! makes capture cheap, and it makes the memory cost exactly the cost of the
//! cells being kept — which is large enough that the budget below is a real
//! limit and not a formality.

use std::sync::Arc;

use crate::sim::worker::SimulationSnapshot;

/// How much of the world's history is kept, by default.
///
/// A 256x256 single-channel frame is a quarter of a megabyte, so this is a few
/// thousand frames. When the budget is reached the oldest frames are dropped:
/// someone watching for something to happen wants the run up to now, not the
/// run up to when the memory ran out.
pub const DEFAULT_BUDGET_BYTES: usize = 512 * 1024 * 1024;

/// Whether playback is running, and which way.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReplayState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

/// Captured frames and a playhead over them.
pub struct Recording {
    frames: Vec<Arc<SimulationSnapshot>>,
    capturing: bool,
    /// Generation of the last frame taken, so one displayed state is not
    /// recorded many times while the simulation is paused.
    last_generation: Option<u64>,
    budget_bytes: usize,
    bytes: usize,
    /// Frames dropped to stay inside the budget. Reported rather than hidden:
    /// a recording that quietly forgot its beginning would be lying about what
    /// it holds.
    dropped: usize,
    state: ReplayState,
    playhead: usize,
    /// Frames per second of playback, independent of how fast they were made.
    speed: f32,
    /// Frames per second of capture.
    ///
    /// Taking every displayed frame sounds faithful and is not usable: a
    /// backend running at several hundred hertz fills the whole budget in a
    /// few seconds, so a take covers an eyeblink of the run the user wanted.
    /// Sampling at a watchable rate is what makes a take describe a run.
    capture_rate: f32,
    /// Simulated time carried between captures.
    capture_accumulator: f64,
    /// Fractional frames carried between display frames.
    accumulator: f64,
}

impl Default for Recording {
    fn default() -> Self {
        Self {
            frames: Vec::new(),
            capturing: false,
            last_generation: None,
            budget_bytes: DEFAULT_BUDGET_BYTES,
            bytes: 0,
            dropped: 0,
            state: ReplayState::Stopped,
            playhead: 0,
            speed: 30.0,
            capture_rate: 30.0,
            capture_accumulator: 0.0,
            accumulator: 0.0,
        }
    }
}

impl Recording {
    pub fn with_budget(budget_bytes: usize) -> Self {
        Self {
            budget_bytes,
            ..Self::default()
        }
    }

    pub fn is_capturing(&self) -> bool {
        self.capturing
    }

    pub fn frames(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn state(&self) -> ReplayState {
        self.state
    }

    pub fn playhead(&self) -> usize {
        self.playhead
    }

    pub fn speed(&self) -> f32 {
        self.speed
    }

    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed.clamp(1.0, 240.0);
    }

    pub fn capture_rate(&self) -> f32 {
        self.capture_rate
    }

    pub fn set_capture_rate(&mut self, rate: f32) {
        self.capture_rate = rate.clamp(1.0, 240.0);
    }

    /// Let enough time pass for the next frame to be wanted.
    ///
    /// Called once per displayed frame with the time since the last one. The
    /// budget is spent on frames far enough apart to be worth keeping.
    pub fn tick_capture_clock(&mut self, dt: f64) -> bool {
        if !self.capturing {
            return false;
        }
        self.capture_accumulator += dt;
        let interval = 1.0 / self.capture_rate.max(1.0) as f64;
        if self.capture_accumulator < interval {
            return false;
        }
        // Drop whole intervals rather than accumulating a backlog, so a slow
        // frame does not cause a burst of captures afterwards.
        self.capture_accumulator = 0.0;
        true
    }

    /// Start capturing. Recording always continues an existing take rather than
    /// silently discarding it; clearing is a separate, named action.
    pub fn start(&mut self) {
        self.capturing = true;
        self.state = ReplayState::Stopped;
    }

    pub fn stop(&mut self) {
        self.capturing = false;
    }

    pub fn clear(&mut self) {
        self.frames.clear();
        self.capture_accumulator = 0.0;
        self.bytes = 0;
        self.dropped = 0;
        self.playhead = 0;
        self.last_generation = None;
        self.state = ReplayState::Stopped;
    }

    /// Take a frame, if one is wanted and this state has not been taken.
    pub fn capture(&mut self, snapshot: Arc<SimulationSnapshot>) {
        if !self.capturing {
            return;
        }
        if self.last_generation == Some(snapshot.generation) {
            return;
        }
        self.last_generation = Some(snapshot.generation);
        let cost = frame_bytes(&snapshot);
        self.bytes += cost;
        self.frames.push(snapshot);
        // Drop from the front until the take fits. A single frame larger than
        // the whole budget is still kept: refusing to record anything at all
        // would be a worse answer than holding one frame.
        while self.bytes > self.budget_bytes && self.frames.len() > 1 {
            let removed = self.frames.remove(0);
            self.bytes = self.bytes.saturating_sub(frame_bytes(&removed));
            self.dropped += 1;
            self.playhead = self.playhead.saturating_sub(1);
        }
    }

    /// Whether the canvas should show a recorded frame instead of the live world.
    pub fn is_replaying(&self) -> bool {
        !self.frames.is_empty() && self.state != ReplayState::Stopped
    }

    /// The frame under the playhead.
    pub fn current(&self) -> Option<Arc<SimulationSnapshot>> {
        self.frames.get(self.playhead).cloned()
    }

    /// Begin playback, which stops capture: a replay is not new history.
    pub fn play(&mut self) {
        if self.frames.is_empty() {
            return;
        }
        self.capturing = false;
        self.state = ReplayState::Playing;
        // Starting from the end would show one frame and stop, which reads as
        // playback being broken.
        if self.playhead + 1 >= self.frames.len() {
            self.playhead = 0;
        }
    }

    pub fn pause(&mut self) {
        if self.state == ReplayState::Playing {
            self.state = ReplayState::Paused;
        }
    }

    /// Leave replay and go back to the live world.
    pub fn resume_live(&mut self) {
        self.state = ReplayState::Stopped;
    }

    /// Put the playhead somewhere, entering replay if it was not already.
    pub fn seek(&mut self, index: usize) {
        if self.frames.is_empty() {
            return;
        }
        self.playhead = index.min(self.frames.len() - 1);
        if self.state == ReplayState::Stopped {
            self.state = ReplayState::Paused;
        }
        self.capturing = false;
    }

    /// Move by whole frames, for a user stepping through by hand.
    pub fn nudge(&mut self, delta: i64) {
        if self.frames.is_empty() {
            return;
        }
        let last = self.frames.len() as i64 - 1;
        let next = (self.playhead as i64 + delta).clamp(0, last);
        self.seek(next as usize);
        self.state = ReplayState::Paused;
    }

    /// Advance playback by a display frame's worth of time.
    ///
    /// Playback runs at its own rate rather than the rate the frames were
    /// captured at, so a run recorded at 400 Hz is watchable.
    pub fn advance(&mut self, dt: f64) {
        if self.state != ReplayState::Playing || self.frames.is_empty() {
            return;
        }
        self.accumulator += dt * self.speed as f64;
        let whole = self.accumulator.floor();
        if whole < 1.0 {
            return;
        }
        self.accumulator -= whole;
        self.playhead += whole as usize;
        if self.playhead >= self.frames.len() {
            // Loop, so a short take does not end on a still frame the user has
            // to rewind by hand.
            self.playhead %= self.frames.len();
        }
    }

    /// The tick of the frame under the playhead.
    pub fn current_tick(&self) -> Option<u64> {
        self.frames.get(self.playhead).map(|frame| frame.tick)
    }

    /// How the take reads in the status line.
    pub fn summary(&self) -> String {
        let megabytes = self.bytes as f64 / (1024.0 * 1024.0);
        // The capture rate has its own control beside this; repeating it here
        // only made the line wrap.
        let mut text = format!("{} frames · {megabytes:.0} MB", self.frames.len());
        if self.dropped > 0 {
            text.push_str(&format!(" · {} dropped", self.dropped));
        }
        text
    }
}

/// Bytes one frame holds, which is dominated by its cells.
fn frame_bytes(snapshot: &SimulationSnapshot) -> usize {
    snapshot.cells.len() * std::mem::size_of::<f32>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::basis_runtime::StateLayout;
    use crate::sim::local_backend::BackendDescriptor;
    use crate::sim::local_backend::StepStats;
    use crate::sim::tiling::BasisId;

    fn frame(generation: u64, tick: u64, cells: usize) -> Arc<SimulationSnapshot> {
        Arc::new(SimulationSnapshot {
            generation,
            tick,
            running: true,
            backend: BackendDescriptor::cpu(),
            layout: StateLayout::new(cells, 1, vec![BasisId(0)], 1).unwrap(),
            cells: vec![0.0; cells].into(),
            step_stats: StepStats::default(),
            error: None,
        })
    }

    #[test]
    fn nothing_is_captured_until_recording_starts() {
        let mut recording = Recording::default();
        recording.capture(frame(1, 1, 4));
        assert_eq!(recording.frames(), 0);
        recording.start();
        recording.capture(frame(2, 2, 4));
        assert_eq!(recording.frames(), 1);
    }

    #[test]
    fn one_displayed_state_is_recorded_once() {
        let mut recording = Recording::default();
        recording.start();
        recording.capture(frame(7, 1, 4));
        recording.capture(frame(7, 1, 4));
        assert_eq!(
            recording.frames(),
            1,
            "a paused simulation republishes the same generation every frame"
        );
    }

    #[test]
    fn the_oldest_frames_are_dropped_when_the_budget_is_reached() {
        // Four cells is sixteen bytes a frame.
        let mut recording = Recording::with_budget(48);
        recording.start();
        for generation in 0..10 {
            recording.capture(frame(generation, generation, 4));
        }
        assert!(recording.frames() <= 3, "{}", recording.frames());
        assert!(recording.dropped() > 0);
        assert!(
            recording.summary().contains("dropped"),
            "a take that forgot part of itself has to say so: {}",
            recording.summary()
        );
    }

    #[test]
    fn playback_stops_capture_so_a_replay_is_not_recorded_as_new_history() {
        let mut recording = Recording::default();
        recording.start();
        recording.capture(frame(1, 1, 4));
        recording.play();
        assert!(!recording.is_capturing());
        assert!(recording.is_replaying());
    }

    #[test]
    fn playback_advances_at_its_own_rate_and_loops() {
        let mut recording = Recording::default();
        recording.start();
        for generation in 0..4 {
            recording.capture(frame(generation, generation, 4));
        }
        recording.set_speed(10.0);
        recording.play();
        recording.advance(0.1);
        assert_eq!(recording.playhead(), 1);
        recording.advance(0.4);
        assert_eq!(recording.playhead(), 1, "four frames later it wraps around");
    }

    #[test]
    fn seeking_enters_replay_and_holds_the_frame() {
        let mut recording = Recording::default();
        recording.start();
        for generation in 0..5 {
            recording.capture(frame(generation, generation * 3, 4));
        }
        recording.seek(3);
        assert_eq!(recording.state(), ReplayState::Paused);
        assert_eq!(recording.current_tick(), Some(9));
    }

    #[test]
    fn stepping_stays_inside_the_take() {
        let mut recording = Recording::default();
        recording.start();
        for generation in 0..3 {
            recording.capture(frame(generation, generation, 4));
        }
        recording.nudge(-10);
        assert_eq!(recording.playhead(), 0);
        recording.nudge(10);
        assert_eq!(recording.playhead(), 2);
    }

    #[test]
    fn returning_to_live_leaves_the_frames_alone() {
        let mut recording = Recording::default();
        recording.start();
        recording.capture(frame(1, 1, 4));
        recording.seek(0);
        recording.resume_live();
        assert!(!recording.is_replaying());
        assert_eq!(recording.frames(), 1, "going back to live is not a delete");
    }

    #[test]
    fn clearing_empties_the_take() {
        let mut recording = Recording::default();
        recording.start();
        recording.capture(frame(1, 1, 4));
        recording.clear();
        assert!(recording.is_empty());
        assert_eq!(recording.bytes(), 0);
    }
}

#[cfg(test)]
mod capture_rate_tests {
    use super::*;
    use crate::sim::basis_runtime::StateLayout;
    use crate::sim::local_backend::BackendDescriptor;
    use crate::sim::local_backend::StepStats;
    use crate::sim::tiling::BasisId;

    fn frame(generation: u64) -> Arc<SimulationSnapshot> {
        Arc::new(SimulationSnapshot {
            generation,
            tick: generation,
            running: true,
            backend: BackendDescriptor::cpu(),
            layout: StateLayout::new(4, 1, vec![BasisId(0)], 1).unwrap(),
            cells: vec![0.0; 4].into(),
            step_stats: StepStats::default(),
            error: None,
        })
    }

    #[test]
    fn the_capture_clock_paces_frames_rather_than_taking_every_one() {
        let mut recording = Recording::default();
        recording.start();
        recording.set_capture_rate(10.0);
        // Sixty display frames in one second at a hundredth of a second each.
        let mut taken = 0;
        for generation in 0..100 {
            if recording.tick_capture_clock(0.01) {
                recording.capture(frame(generation));
                taken += 1;
            }
        }
        assert!(
            (9..=11).contains(&taken),
            "one second at ten per second is about ten frames, not one hundred: {taken}"
        );
    }

    #[test]
    fn the_clock_only_runs_while_recording() {
        let mut recording = Recording::default();
        assert!(!recording.tick_capture_clock(10.0));
    }
}
