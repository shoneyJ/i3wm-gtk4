//! Voice-activity detection — energy-based, ported from
//! `vendor/whisper.cpp/examples/common.cpp` (`high_pass_filter` ~line 597
//! and `vad_simple` ~line 610). Pure Rust, no FFI.
//!
//! Two functions exposed:
//!
//! - [`is_chunk_silent`] — answers "is this whole chunk basically empty?"
//!   Used by the inference worker to skip whisper entirely on silent
//!   chunks, which is what kills the `* Musik *` hallucination on
//!   un-played audio. This is the function that powers Phase S3.
//!
//! - [`is_speech_end`] — answers "did the speaker just stop talking?"
//!   Returns `true` when the recent `last_ms` window is significantly
//!   quieter than the chunk average. This is the classic whisper.cpp VAD,
//!   useful for committing final segments at speech boundaries — needed
//!   by Phase S2 (sliding window) but unused by S3 itself. Kept here so
//!   S2 doesn't need to re-port the C++.

/// Default Hz; matches whisper.cpp's stream example default.
pub const DEFAULT_FREQ_THOLD_HZ: f32 = 100.0;

/// Mean |amplitude| (in normalised f32 [-1, 1]) below which a chunk is
/// treated as silent. Calibrated empirically against the Jabra A2DP
/// monitor + Tagesschau speech: real speech mean |s| ≈ 0.03–0.07; noise
/// floor of an idle-but-running BT monitor ≈ 0.0001–0.001. Threshold
/// 0.003 sits comfortably between, with margin for whispers / quiet
/// passages.
pub const DEFAULT_SILENCE_ENERGY_THOLD: f32 = 0.003;

/// First-order high-pass filter, single-pass, in-place. Removes DC and
/// low-frequency rumble (e.g. handling noise) before energy measurement.
/// Direct port of the C++ original.
pub fn high_pass_filter(data: &mut [f32], cutoff_hz: f32, sample_rate: u32) {
    if data.is_empty() || cutoff_hz <= 0.0 {
        return;
    }
    let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff_hz);
    let dt = 1.0 / sample_rate as f32;
    let alpha = dt / (rc + dt);

    let mut y = data[0];
    for i in 1..data.len() {
        y = alpha * (y + data[i] - data[i - 1]);
        data[i] = y;
    }
}

/// Returns `true` if the chunk's mean absolute amplitude is below
/// `energy_thold`. Does NOT high-pass — at chunk-mean granularity DC
/// bias is irrelevant, and the upstream whisper.cpp HP coefficient
/// formulation is heavily attenuating (≈ 15× quieter post-filter), which
/// would force the threshold into a band that's noisy to calibrate.
/// `_sample_rate` and `_freq_thold` retained for symmetry with
/// [`is_speech_end`] callers.
pub fn is_chunk_silent(
    samples: &[f32],
    _sample_rate: u32,
    _freq_thold: f32,
    energy_thold: f32,
) -> bool {
    if samples.is_empty() {
        return true;
    }
    let sum: f32 = samples.iter().map(|s| s.abs()).sum();
    let mean = sum / samples.len() as f32;
    mean < energy_thold
}

/// Whisper.cpp's `vad_simple` ported verbatim — mutates the buffer (high-pass).
/// Returns `true` when the recent `last_ms` window is *quieter* than
/// `vad_thold * average_chunk_energy`, i.e. "the speaker has stopped."
///
/// Suitable for end-of-utterance detection inside a sliding window. NOT
/// used by Phase S3 (which uses [`is_chunk_silent`] instead). Kept here
/// because Phase S2's segment-commit logic will need exactly this.
#[allow(dead_code)]
pub fn is_speech_end(
    samples: &mut [f32],
    sample_rate: u32,
    last_ms: u32,
    vad_thold: f32,
    freq_thold: f32,
) -> bool {
    let n_samples = samples.len();
    let n_samples_last = (sample_rate as usize * last_ms as usize) / 1000;
    if n_samples_last >= n_samples {
        return false;
    }
    if freq_thold > 0.0 {
        high_pass_filter(samples, freq_thold, sample_rate);
    }
    let mut energy_all = 0.0f32;
    let mut energy_last = 0.0f32;
    for (i, s) in samples.iter().enumerate() {
        energy_all += s.abs();
        if i >= n_samples - n_samples_last {
            energy_last += s.abs();
        }
    }
    energy_all /= n_samples as f32;
    energy_last /= n_samples_last as f32;
    energy_last <= vad_thold * energy_all
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_silence(n: usize) -> Vec<f32> {
        vec![0.0; n]
    }

    fn make_tone(n: usize, sample_rate: u32, hz: f32, amp: f32) -> Vec<f32> {
        let dt = 1.0 / sample_rate as f32;
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * hz * i as f32 * dt).sin())
            .collect()
    }

    #[test]
    fn silence_chunk_is_silent() {
        let s = make_silence(16_000);
        assert!(is_chunk_silent(
            &s,
            16_000,
            DEFAULT_FREQ_THOLD_HZ,
            DEFAULT_SILENCE_ENERGY_THOLD,
        ));
    }

    #[test]
    fn loud_tone_is_not_silent() {
        let s = make_tone(16_000, 16_000, 440.0, 0.05);
        assert!(!is_chunk_silent(
            &s,
            16_000,
            DEFAULT_FREQ_THOLD_HZ,
            DEFAULT_SILENCE_ENERGY_THOLD,
        ));
    }

    #[test]
    fn very_quiet_tone_is_silent() {
        // Below threshold (0.003 mean) — synthesise a tone scaled to
        // amplitude 0.002 → mean |s| ≈ 0.00127 < 0.003.
        let s = make_tone(16_000, 16_000, 440.0, 0.002);
        assert!(is_chunk_silent(
            &s,
            16_000,
            DEFAULT_FREQ_THOLD_HZ,
            DEFAULT_SILENCE_ENERGY_THOLD,
        ));
    }
}
