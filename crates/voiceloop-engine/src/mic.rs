//! "Mic only when needed": which reasons hold the input stream open, when it
//! opens and closes, and the recording/calibration requests that wait for it.
//!
//! Pure control-thread logic (no streams), unit-tested here. `engine.rs`
//! applies the decisions; `hardware.rs` opens and closes the input stream;
//! `looper/input_link.rs` attaches it to the running output.
//!
//! Reasons come from two places:
//! - the frontend's declared needs ([`InputNeeds`], `set_input_needs`): live
//!   pitch / tuner, Practice Lab, the "always on" mic mode, ...
//! - the engine itself: a looper track armed, recording or overdubbing (or a
//!   record command waiting for the mic), monitoring on, a calibration, an
//!   active Practice take.
//!
//! The input opens as soon as any reason exists and closes once none has
//! existed for the hold time (`audio.micIdleCloseSeconds`, default 10 s).

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::looper::LooperCommand;

/// Default for how long the mic stays open after the last reason goes away.
pub const DEFAULT_HOLD_MS: u32 = 10_000;
/// Upper bound for the hold (keeps a bad value from pinning the mic open for hours).
pub const MAX_HOLD_MS: u32 = 600_000;
/// Recording or calibration waiting for the mic gives up after this long.
pub const START_TIMEOUT: Duration = Duration::from_secs(5);
/// After the input failed to open, wait this long before trying again.
pub const RETRY_AFTER: Duration = Duration::from_secs(3);

/// Why the microphone is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InputReason {
    /// Mic mode "always on" (frontend setting `audio.micMode`).
    AlwaysOn,
    /// Live pitch, tuner, or pitch-reactive visuals.
    Pitch,
    /// Practice Lab (frontend: the workspace is open; engine: a take is active).
    Practice,
    /// Games that listen to the microphone.
    Game,
    /// Pocket Song Builder is listening for voice drums.
    SongBuilder,
    /// The input level meter is wanted (e.g. the input settings panel is open).
    Meter,
    /// Engine: a track is armed, recording or overdubbing, or a record command waits.
    Record,
    /// Engine: input monitoring is on.
    Monitor,
    /// Engine: a loopback latency calibration runs or waits.
    Calibration,
    /// Engine: "Learn room" is recording the room's noise.
    Noise,
}

impl InputReason {
    /// Reasons only the engine sets (ignored when the frontend sends them).
    pub fn engine_owned(self) -> bool {
        matches!(self, Self::Record | Self::Monitor | Self::Calibration | Self::Noise)
    }
}

/// What the frontend needs the mic for (`set_input_needs`). Replaces the previous set.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InputNeeds {
    pub reasons: Vec<InputReason>,
    /// Keep the mic open this long after the last reason ends (`None` = unchanged).
    pub hold_ms: Option<u32>,
}

/// Microphone (input stream) state as the UI shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MicState {
    /// Closed: the system mic indicator is off.
    Off,
    /// Open, input not reaching the looper yet.
    Opening,
    /// Open and delivering input.
    On,
}

/// `EngineStatus.input`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputStatus {
    pub mic: MicState,
    /// Every reason holding the mic open now (frontend and engine), sorted.
    pub reasons: Vec<InputReason>,
    pub hold_ms: u32,
    /// Nothing needs the mic: it closes in this many ms (`None` otherwise).
    pub closing_in_ms: Option<u32>,
    /// Record / overdub / calibration requests waiting for the mic to start.
    pub waiting: u32,
}

impl InputStatus {
    /// Same state for the UI (ignoring the countdown value).
    pub fn same_as(&self, other: &Self) -> bool {
        self.mic == other.mic
            && self.reasons == other.reasons
            && self.hold_ms == other.hold_ms
            && self.closing_in_ms.is_some() == other.closing_in_ms.is_some()
            && self.waiting == other.waiting
    }
}

/// What the control loop must do with the input stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicAction {
    Open,
    Close,
    None,
}

/// Reasons and the release timer.
#[derive(Debug, Clone)]
pub struct MicDemand {
    requested: Vec<InputReason>,
    hold: Duration,
    idle_since: Option<Instant>,
}

impl Default for MicDemand {
    fn default() -> Self {
        Self { requested: Vec::new(), hold: Duration::from_millis(DEFAULT_HOLD_MS as u64), idle_since: None }
    }
}

impl MicDemand {
    /// Apply the frontend's declared needs (engine-owned reasons are dropped).
    pub fn set_needs(&mut self, needs: &InputNeeds) {
        let mut reasons: Vec<InputReason> = needs.reasons.iter().copied().filter(|r| !r.engine_owned()).collect();
        reasons.sort();
        reasons.dedup();
        self.requested = reasons;
        if let Some(ms) = needs.hold_ms {
            self.hold = Duration::from_millis(ms.min(MAX_HOLD_MS) as u64);
        }
    }

    pub fn hold_ms(&self) -> u32 {
        self.hold.as_millis() as u32
    }

    /// Frontend reasons plus `engine` reasons, sorted, without duplicates.
    pub fn reasons(&self, engine: &[InputReason]) -> Vec<InputReason> {
        let mut all: Vec<InputReason> = self.requested.iter().chain(engine).copied().collect();
        all.sort();
        all.dedup();
        all
    }

    /// Decide for this moment. `open`: the input stream is open now.
    pub fn decide(&mut self, engine: &[InputReason], open: bool, now: Instant) -> MicAction {
        if !self.requested.is_empty() || !engine.is_empty() {
            self.idle_since = None;
            return if open { MicAction::None } else { MicAction::Open };
        }
        if !open {
            self.idle_since = None;
            return MicAction::None;
        }
        let since = *self.idle_since.get_or_insert(now);
        if now.saturating_duration_since(since) >= self.hold {
            self.idle_since = None;
            MicAction::Close
        } else {
            MicAction::None
        }
    }

    /// Time left before the mic closes, while nothing needs it.
    pub fn closing_in(&self, now: Instant) -> Option<Duration> {
        self.idle_since.map(|since| self.hold.saturating_sub(now.saturating_duration_since(since)))
    }
}

/// Requests that need input flowing, held until it does ("arm and wait").
#[derive(Debug, Default)]
pub struct MicWaiting {
    looper: Vec<LooperCommand>,
    /// A calibration to start, with its `apply` flag.
    calibration: Option<bool>,
    since: Option<Instant>,
}

/// What [`MicWaiting::take`] released.
#[derive(Debug, Default, PartialEq)]
pub struct Released {
    pub looper: Vec<LooperCommand>,
    pub calibration: Option<bool>,
    /// The mic did not start in time: these were dropped, not released.
    pub timed_out: bool,
}

impl MicWaiting {
    pub fn is_empty(&self) -> bool {
        self.looper.is_empty() && self.calibration.is_none()
    }

    pub fn count(&self) -> u32 {
        self.looper.len() as u32 + self.calibration.is_some() as u32
    }

    pub fn has_looper(&self) -> bool {
        !self.looper.is_empty()
    }

    pub fn has_calibration(&self) -> bool {
        self.calibration.is_some()
    }

    pub fn push_looper(&mut self, command: LooperCommand, now: Instant) {
        self.since.get_or_insert(now);
        self.looper.push(command);
    }

    pub fn push_calibration(&mut self, apply: bool, now: Instant) {
        self.since.get_or_insert(now);
        self.calibration = Some(apply);
    }

    /// Whether a looper command must wait behind earlier ones (keeps order).
    pub fn must_queue(&self, command: &LooperCommand) -> bool {
        self.has_looper() && Self::track_of(command).is_some()
    }

    fn track_of(command: &LooperCommand) -> Option<u8> {
        match *command {
            LooperCommand::Record { track } | LooperCommand::Overdub { track } => Some(track),
            _ => None,
        }
    }

    /// A command arrived while requests wait. Returns true when it only
    /// cancelled a waiting request (and must not be forwarded).
    pub fn on_command(&mut self, command: &LooperCommand) -> bool {
        if self.looper.is_empty() {
            return false;
        }
        let consumed = match *command {
            LooperCommand::StopRecording { track } => {
                let before = self.looper.len();
                self.looper.retain(|c| Self::track_of(c) != Some(track));
                before != self.looper.len()
            }
            LooperCommand::Clear { track } => {
                self.looper.retain(|c| Self::track_of(c) != Some(track));
                false
            }
            LooperCommand::Stop
            | LooperCommand::Restart
            | LooperCommand::ClearAll
            | LooperCommand::RemoveTrack { .. } => {
                self.looper.clear();
                false
            }
            _ => false,
        };
        if self.is_empty() {
            self.since = None;
        }
        consumed
    }

    /// Release everything once input flows; drop everything after [`START_TIMEOUT`].
    pub fn take(&mut self, flowing: bool, now: Instant) -> Released {
        let Some(since) = self.since else { return Released::default() };
        if flowing || now.saturating_duration_since(since) >= START_TIMEOUT {
            self.since = None;
            return Released {
                looper: std::mem::take(&mut self.looper),
                calibration: self.calibration.take(),
                timed_out: !flowing,
            };
        }
        Released::default()
    }

    /// Audio stopped: forget everything.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn opens_for_any_reason_and_closes_after_the_hold() {
        let t0 = Instant::now();
        let mut d = MicDemand::default();
        assert_eq!(d.decide(&[], false, t0), MicAction::None, "nothing needs it: stays closed");
        d.set_needs(&InputNeeds { reasons: vec![InputReason::Pitch], hold_ms: Some(2_000) });
        assert_eq!(d.decide(&[], false, t0), MicAction::Open);
        assert_eq!(d.decide(&[], true, at(t0, 10)), MicAction::None);
        d.set_needs(&InputNeeds { reasons: vec![], hold_ms: None });
        assert_eq!(d.hold_ms(), 2_000, "hold unchanged when omitted");
        assert_eq!(d.decide(&[], true, at(t0, 100)), MicAction::None, "held");
        assert_eq!(d.closing_in(at(t0, 600)), Some(Duration::from_millis(1_500)));
        assert_eq!(d.decide(&[], true, at(t0, 2_099)), MicAction::None);
        assert_eq!(d.decide(&[], true, at(t0, 2_100)), MicAction::Close);
        assert_eq!(d.closing_in(at(t0, 2_100)), None);
    }

    #[test]
    fn a_new_reason_during_the_hold_keeps_the_mic_open() {
        let t0 = Instant::now();
        let mut d = MicDemand::default();
        assert_eq!(d.decide(&[InputReason::Record], false, t0), MicAction::Open);
        assert_eq!(d.decide(&[], true, at(t0, 1_000)), MicAction::None);
        assert_eq!(d.decide(&[InputReason::Monitor], true, at(t0, 9_000)), MicAction::None);
        assert_eq!(d.closing_in(at(t0, 9_000)), None, "timer reset");
        assert_eq!(d.decide(&[], true, at(t0, 12_000)), MicAction::None, "full hold again from 12 s");
        assert_eq!(d.decide(&[], true, at(t0, 22_000)), MicAction::Close);
    }

    #[test]
    fn zero_hold_closes_at_once_and_the_hold_is_bounded() {
        let t0 = Instant::now();
        let mut d = MicDemand::default();
        d.set_needs(&InputNeeds { reasons: vec![], hold_ms: Some(0) });
        assert_eq!(d.decide(&[], true, t0), MicAction::Close);
        d.set_needs(&InputNeeds { reasons: vec![], hold_ms: Some(u32::MAX) });
        assert_eq!(d.hold_ms(), MAX_HOLD_MS);
    }

    #[test]
    fn frontend_cannot_set_engine_owned_reasons() {
        let mut d = MicDemand::default();
        d.set_needs(&InputNeeds {
            reasons: vec![InputReason::Record, InputReason::Pitch, InputReason::Pitch, InputReason::AlwaysOn],
            hold_ms: None,
        });
        assert_eq!(d.reasons(&[]), vec![InputReason::AlwaysOn, InputReason::Pitch]);
        assert_eq!(
            d.reasons(&[InputReason::Monitor, InputReason::Pitch]),
            vec![InputReason::AlwaysOn, InputReason::Pitch, InputReason::Monitor]
        );
    }

    #[test]
    fn waiting_records_are_released_in_order_once_input_flows() {
        let t0 = Instant::now();
        let mut w = MicWaiting::default();
        w.push_looper(LooperCommand::Record { track: 0 }, t0);
        assert!(w.must_queue(&LooperCommand::Overdub { track: 1 }));
        assert!(!w.must_queue(&LooperCommand::SetMute { track: 1, muted: true }));
        w.push_looper(LooperCommand::Overdub { track: 1 }, at(t0, 50));
        assert_eq!(w.count(), 2);
        assert_eq!(w.take(false, at(t0, 100)), Released::default(), "still waiting");
        let r = w.take(true, at(t0, 200));
        assert_eq!(r.looper, vec![LooperCommand::Record { track: 0 }, LooperCommand::Overdub { track: 1 }]);
        assert!(!r.timed_out);
        assert!(w.is_empty());
    }

    #[test]
    fn waiting_requests_time_out_and_can_be_cancelled() {
        let t0 = Instant::now();
        let mut w = MicWaiting::default();
        w.push_looper(LooperCommand::Record { track: 2 }, t0);
        assert!(w.on_command(&LooperCommand::StopRecording { track: 2 }), "stop cancels the waiting record");
        assert!(w.is_empty());
        assert!(!w.on_command(&LooperCommand::StopRecording { track: 2 }), "nothing waits: forwarded");

        w.push_looper(LooperCommand::Record { track: 0 }, t0);
        assert!(!w.on_command(&LooperCommand::Stop), "Stop is forwarded and clears the queue");
        assert!(w.is_empty());

        w.push_calibration(true, t0);
        w.push_looper(LooperCommand::Record { track: 0 }, t0);
        let r = w.take(false, t0 + START_TIMEOUT);
        assert!(r.timed_out);
        assert_eq!((r.looper.len(), r.calibration), (1, Some(true)));
        assert!(w.is_empty());
    }

    #[test]
    fn input_status_serialises_for_the_frontend() {
        let s = InputStatus {
            mic: MicState::Opening,
            reasons: vec![InputReason::AlwaysOn, InputReason::Record],
            hold_ms: 10_000,
            closing_in_ms: None,
            waiting: 1,
        };
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({"mic": "opening", "reasons": ["alwaysOn", "record"], "holdMs": 10000,
                "closingInMs": null, "waiting": 1})
        );
        let needs: InputNeeds = serde_json::from_str(r#"{"reasons":["pitch","practice"],"holdMs":2500}"#).unwrap();
        assert_eq!(needs.reasons, vec![InputReason::Pitch, InputReason::Practice]);
        assert_eq!(needs.hold_ms, Some(2_500));
        let s2 = InputStatus { closing_in_ms: None, ..s.clone() };
        assert!(s.same_as(&s2));
    }
}
