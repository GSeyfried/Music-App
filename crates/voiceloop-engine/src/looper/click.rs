//! Metronome click: synthesised in the output callback, mixed only into the
//! output. It is never written into recordings. (Acoustic bleed from speakers
//! into the microphone is physical; headphones avoid it.)

use crate::transport::TransportState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accent {
    /// Downbeat of a bar.
    Strong,
    /// Other beats.
    Normal,
    /// Subdivisions between beats.
    Weak,
}

/// Accent for subdivision-grid point `index` (0 = bar 1 beat 1).
///
/// This is the single place that maps grid position to accent. Custom accent
/// patterns (e.g. 2+2+3 grouping in 7/8) can be added by replacing this
/// function's body with a pattern lookup, without touching the clock.
pub fn accent_for(index: i64, transport: &TransportState) -> Accent {
    let subs = transport.metronome.subdivision.max(1) as i64;
    let beats_per_bar = transport.time_signature.numerator.max(1) as i64;
    let sub = index.rem_euclid(subs);
    let beat_in_bar = index.div_euclid(subs).rem_euclid(beats_per_bar);
    if sub != 0 {
        Accent::Weak
    } else if beat_in_bar == 0 && transport.metronome.accent_downbeat {
        Accent::Strong
    } else {
        Accent::Normal
    }
}

/// Single decaying-sine click voice. Retriggering restarts it. No allocation.
#[derive(Debug, Clone)]
pub struct ClickVoice {
    sample_rate: f32,
    phase: f32,
    increment: f32,
    amplitude: f32,
    decay: f32,
    remaining: u32,
}

impl ClickVoice {
    /// Click length in seconds.
    const LENGTH_S: f32 = 0.03;
    /// Exponential decay time constant in seconds.
    const TAU_S: f32 = 0.006;

    pub fn new(sample_rate: u32) -> Self {
        Self { sample_rate: sample_rate.max(1) as f32, phase: 0.0, increment: 0.0, amplitude: 0.0, decay: 0.0, remaining: 0 }
    }

    pub fn trigger(&mut self, accent: Accent, gain: f32) {
        let (freq, level) = match accent {
            Accent::Strong => (1760.0, 1.0),
            Accent::Normal => (1320.0, 0.7),
            Accent::Weak => (880.0, 0.45),
        };
        // Start at the sine peak so the first sample is audible and deterministic.
        self.phase = 0.25;
        self.increment = freq / self.sample_rate;
        self.amplitude = gain * level;
        self.decay = (-1.0 / (Self::TAU_S * self.sample_rate)).exp();
        self.remaining = (Self::LENGTH_S * self.sample_rate) as u32;
    }

    #[inline]
    pub fn next_sample(&mut self) -> f32 {
        if self.remaining == 0 {
            return 0.0;
        }
        let s = (self.phase * std::f32::consts::TAU).sin() * self.amplitude;
        self.phase += self.increment;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        self.amplitude *= self.decay;
        self.remaining -= 1;
        s
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TimeSignature;

    #[test]
    fn accents_follow_meter_and_subdivision() {
        let mut t = TransportState { time_signature: TimeSignature::new(7, 8).unwrap(), ..Default::default() };
        t.metronome.subdivision = 2;
        assert_eq!(accent_for(0, &t), Accent::Strong);
        assert_eq!(accent_for(1, &t), Accent::Weak);
        assert_eq!(accent_for(2, &t), Accent::Normal);
        assert_eq!(accent_for(14, &t), Accent::Strong); // bar 2
        t.metronome.accent_downbeat = false;
        assert_eq!(accent_for(0, &t), Accent::Normal);
    }

    #[test]
    fn click_starts_at_peak_and_ends() {
        let mut v = ClickVoice::new(1_000);
        v.trigger(Accent::Strong, 1.0);
        assert!((v.next_sample() - 1.0).abs() < 1e-6);
        for _ in 0..40 {
            v.next_sample();
        }
        assert_eq!(v.next_sample(), 0.0);
    }
}
