//! Speech-to-text runtime.
//!
//! Captures audio from a PulseAudio sink's monitor via `parec`, feeds fixed-length
//! PCM chunks to an in-process whisper.cpp context (via `whisper-rs`), and emits
//! transcribed `Segment`s on an mpsc channel.
//!
//! Single child process: `parec`. Model is loaded once in the worker thread and
//! reused across chunks. CUDA backend is picked at `WhisperContext` init when the
//! crate is built with the `cuda` feature.

use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub mod capture;
mod parec;
mod pipewire;
mod vad;

const SAMPLE_RATE: u32 = 16_000;

/// Whether a segment is in-flight or committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    /// Mid-utterance — the same speech is still being heard, the text may
    /// extend or be revised on the next step.
    Provisional,
    /// Speaker has paused (VAD-detected end-of-speech) or the underlying
    /// chunk fell silent. Final text is locked in. Only Final segments
    /// are written to the transcript file and translated.
    Final,
}

/// A single transcribed chunk.
#[derive(Debug, Clone)]
pub struct Segment {
    /// Wall-clock timestamp when the chunk finished being captured.
    pub at: std::time::SystemTime,
    /// Source-language text (e.g. German).
    pub text: String,
    /// Translation to `translate_target` if enabled and successful.
    /// Always `None` for Provisional; only set for Final after translation.
    pub translation: Option<String>,
    /// Provisional vs Final.
    pub kind: SegmentKind,
}

/// Runtime configuration, persisted at `~/.config/i3more/speech-text.json`.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpeechTextConfig {
    /// Absolute path to a whisper.cpp ggml model file.
    pub model_path: PathBuf,
    /// Source language code (e.g. `"de"`).
    pub language: String,
    /// Substring matched (case-insensitive) against each sink's description.
    /// **Empty string** (the default) means "follow whatever PulseAudio /
    /// PipeWire reports as the default sink" — so audio-control-panel
    /// switches are honoured automatically. Set to e.g. `"jabra"` to pin a
    /// session to a specific device regardless of the system default.
    #[serde(default)]
    pub device_match: String,
    /// Capture backend: `"parec"` (default — works on every Bluetooth
    /// profile including HSP/HFP) or `"pipewire"` (native client; lower
    /// overhead but currently silent on HSP/HFP — see
    /// `src/speech_text/pipewire.rs` for the regression note).
    #[serde(default = "default_capture_backend")]
    pub capture_backend: String,
    /// (Legacy v1 tumble-window field — superseded by `length_ms` /
    /// `step_ms`. Retained so existing config files keep parsing. If
    /// `length_ms` is left at default, this scales it.)
    pub chunk_seconds: u32,
    /// Sliding-window length in ms — the inference operates on the last
    /// `length_ms` of audio every step. 8000 ms gives whisper enough
    /// context for accurate German without being silly slow on the small
    /// model.
    #[serde(default = "default_length_ms")]
    pub length_ms: u32,
    /// Step size in ms — how often inference re-runs. Smaller = lower
    /// perceived latency, higher CPU/GPU pressure. 1500 ms keeps real-time
    /// factor margin on the small model on an RTX 4050.
    #[serde(default = "default_step_ms")]
    pub step_ms: u32,
    /// VAD energy ratio threshold for end-of-speech detection. The last
    /// 500 ms of the window is compared to the whole window's energy; if
    /// `last < vad_thold * all`, treat as speaker-paused.
    #[serde(default = "default_vad_thold")]
    pub vad_thold: f32,
    /// Threads used by whisper on CPU fallback; ignored when CUDA is used.
    pub threads: u32,
    /// If `true` (default), translate each transcribed segment from
    /// `language` to `translate_target` using the existing `trans` CLI
    /// (see `src/translate.rs`). Adds ~1–2 s per segment.
    #[serde(default = "default_true")]
    pub translate_enabled: bool,
    /// Target language for inline translation when `translate_enabled`.
    #[serde(default = "default_translate_target")]
    pub translate_target: String,
}

fn default_capture_backend() -> String {
    "parec".to_string()
}

fn default_true() -> bool {
    true
}

fn default_translate_target() -> String {
    "en".to_string()
}

fn default_length_ms() -> u32 {
    8000
}

fn default_step_ms() -> u32 {
    1500
}

fn default_vad_thold() -> f32 {
    0.6
}

impl Default for SpeechTextConfig {
    fn default() -> Self {
        let model = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("i3more/models/ggml-base.bin");
        Self {
            model_path: model,
            language: "de".to_string(),
            device_match: String::new(),
            capture_backend: default_capture_backend(),
            chunk_seconds: 5,
            length_ms: default_length_ms(),
            step_ms: default_step_ms(),
            vad_thold: default_vad_thold(),
            threads: 4,
            translate_enabled: true,
            translate_target: default_translate_target(),
        }
    }
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("i3more")
        .join("speech-text.json")
}

pub fn load_config() -> SpeechTextConfig {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => SpeechTextConfig::default(),
    }
}

pub fn save_config(config: &SpeechTextConfig) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(&path, data);
    }
}

/// Identifies a single capture session for transcript persistence.
/// Created once per `SpeechSession::start` invocation.
#[derive(Debug, Clone)]
pub struct SessionMeta {
    /// Filesystem-safe label (e.g. `"pre-refinement"`). Sanitised by
    /// `sanitise_session_name`.
    pub name: String,
    /// Wall-clock start time. Used for the transcript directory date and the
    /// front-matter `started_at` field.
    pub started_at: SystemTime,
}

impl SessionMeta {
    /// Build a SessionMeta from an optional user-supplied name.
    /// Falls back to `untitled-YYYY-MM-DD-HH-MM` when `name` is `None` or empty.
    pub fn new(name: Option<String>) -> Self {
        let started_at = SystemTime::now();
        let name = name
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(|s| sanitise_session_name(&s))
            .unwrap_or_else(|| {
                let LocalDateTime { year, month, day, hour, minute, .. } =
                    local_datetime(started_at);
                format!(
                    "untitled-{:04}-{:02}-{:02}-{:02}-{:02}",
                    year, month, day, hour, minute
                )
            });
        Self { name, started_at }
    }

    /// Absolute path of the transcript file for this session.
    pub fn transcript_path(&self) -> PathBuf {
        let LocalDateTime { year, month, day, .. } = local_datetime(self.started_at);
        transcript_root()
            .join(format!("{:04}-{:02}-{:02}", year, month, day))
            .join(format!("{}.md", self.name))
    }
}

fn transcript_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("i3more")
        .join("stt")
}

/// Replace path-unsafe characters with `-`. Keep `[A-Za-z0-9._-]`.
fn sanitise_session_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    // Collapse multiple `-` into one.
    let mut prev_dash = false;
    let collapsed: String = out
        .chars()
        .filter(|&c| {
            let dash = c == '-';
            let keep = !(dash && prev_dash);
            prev_dash = dash;
            keep
        })
        .collect();
    collapsed.trim_matches('-').to_string()
}


/// An active capture session. Drop joins the inference worker; the PipeWire
/// thread itself runs for the process lifetime (see capture.rs).
pub struct SpeechSession {
    worker: Option<JoinHandle<()>>,
}

impl SpeechSession {
    pub fn start(
        config: SpeechTextConfig,
        session: SessionMeta,
        tx: Sender<Segment>,
    ) -> Result<Self, String> {
        if !config.model_path.exists() {
            return Err(format!(
                "model not found at {}",
                config.model_path.display()
            ));
        }

        // Spawn the capture supervisor. It picks the backend (parec
        // default, pipewire opt-in), resolves the target sink (default
        // sink unless device_match is set), and re-spawns the backend
        // whenever the default sink changes (Phase 7-prime).
        let pcm_rx = capture::start(&config)?;

        log::info!(
            "session: {} → {}",
            session.name,
            session.transcript_path().display()
        );

        let worker = std::thread::spawn(move || {
            if let Err(e) = run_worker(pcm_rx, config, session, tx) {
                log::error!("speech-text worker exited with error: {}", e);
            }
        });

        Ok(Self {
            worker: Some(worker),
        })
    }
}

impl Drop for SpeechSession {
    fn drop(&mut self) {
        // Worker exits when SHUTDOWN is set or when the PipeWire channel
        // disconnects. Waiting briefly here lets the final transcript flush
        // to disk before the process exits.
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}

/// Worker body — Phase S2 sliding-window inference.
///
/// Maintains a ring of the last `length_ms` of audio. Every `step_ms`,
/// runs whisper on the ring, compares the new transcript to the last one,
/// emits only the *new suffix* as a `Provisional` segment. When VAD
/// detects speech end (last 500 ms quieter than chunk average), the
/// current text is locked in as `Final`, the ring is cleared (keeping a
/// 200 ms tail for the next utterance), and the prompt context resets.
fn run_worker(
    pcm_rx: Receiver<Vec<i16>>,
    config: SpeechTextConfig,
    session: SessionMeta,
    tx: Sender<Segment>,
) -> Result<(), String> {
    log::info!("loading whisper model from {}", config.model_path.display());
    let params = WhisperContextParameters::default();
    let ctx = WhisperContext::new_with_params(
        &config.model_path.to_string_lossy(),
        params,
    )
    .map_err(|e| format!("WhisperContext init: {}", e))?;
    let mut state = ctx
        .create_state()
        .map_err(|e| format!("WhisperContext::create_state: {}", e))?;
    log::info!(
        "whisper ready; sliding window length={}ms step={}ms",
        config.length_ms,
        config.step_ms
    );

    let transcript_path = session.transcript_path();
    let mut transcript = open_transcript(
        &transcript_path,
        &session,
        &config.language,
        &config.model_path,
    )?;

    let length_samples = ((SAMPLE_RATE as u64 * config.length_ms as u64) / 1000) as usize;
    let step_samples = ((SAMPLE_RATE as u64 * config.step_ms as u64) / 1000) as usize;
    // Minimum amount of audio to require before the first inference — avoids
    // running whisper on a half-second of speech start.
    let min_first_samples = step_samples.max(SAMPLE_RATE as usize); // ≥ 1 s
    // After committing a Final, keep this much tail in the ring as
    // priming context for the next utterance — matches whisper.cpp
    // stream's `--keep` parameter (200 ms).
    let keep_after_commit_samples = (SAMPLE_RATE as usize) / 5; // 200 ms

    let mut ring: Vec<i16> = Vec::with_capacity(length_samples + 2 * step_samples);
    let mut samples_since_step: usize = 0;
    let mut prev_text = String::new();

    loop {
        if crate::shutdown_requested() {
            log::info!("shutdown flag set; worker exiting");
            // Final flush: if there's any pending provisional text, emit
            // it as Final so the session ends with closed segments.
            if !prev_text.is_empty() {
                let captured_at = SystemTime::now();
                let translation = maybe_translate(&prev_text, &config);
                append_segment(
                    &mut transcript,
                    captured_at,
                    &prev_text,
                    translation.as_deref(),
                );
                let _ = tx.send(Segment {
                    at: captured_at,
                    text: prev_text.clone(),
                    translation,
                    kind: SegmentKind::Final,
                });
            }
            break;
        }

        // Drain incoming PCM into the ring; cap size by trimming oldest.
        match pcm_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(samples) => {
                samples_since_step += samples.len();
                ring.extend_from_slice(&samples);
                if ring.len() > length_samples {
                    let drop_n = ring.len() - length_samples;
                    ring.drain(..drop_n);
                }
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                log::info!("pipewire channel disconnected; worker exiting");
                break;
            }
        }

        // Step gate.
        if samples_since_step < step_samples || ring.len() < min_first_samples {
            continue;
        }
        samples_since_step = 0;

        let snapshot: Vec<f32> = ring.iter().map(|&s| (s as f32) / 32768.0).collect();
        let mean: f32 = snapshot.iter().map(|s| s.abs()).sum::<f32>() / snapshot.len() as f32;

        // Whole-window silence — emit any pending Provisional as Final
        // (speaker stopped before we noticed via VAD), then skip inference.
        if vad::is_chunk_silent(
            &snapshot,
            SAMPLE_RATE,
            vad::DEFAULT_FREQ_THOLD_HZ,
            vad::DEFAULT_SILENCE_ENERGY_THOLD,
        ) {
            log::debug!("vad: window silent (mean |s|={:.5})", mean);
            if !prev_text.is_empty() {
                emit_final(
                    &mut transcript,
                    &tx,
                    std::mem::take(&mut prev_text),
                    &config,
                );
                ring.clear();
            }
            continue;
        }

        // Inference.
        let t0 = Instant::now();
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(&config.language));
        params.set_n_threads(config.threads as i32);
        params.set_translate(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_no_speech_thold(0.6);
        params.set_suppress_blank(true);
        params.set_temperature(0.0);
        params.set_no_context(true);

        if let Err(e) = state.full(params, &snapshot) {
            log::error!("whisper inference: {}", e);
            continue;
        }
        let took = t0.elapsed();

        let num = state.full_n_segments().unwrap_or(0);
        let mut combined = String::new();
        for i in 0..num {
            if let Ok(seg) = state.full_get_segment_text(i) {
                combined.push_str(seg.trim());
                combined.push(' ');
            }
        }
        let text_new = combined.trim().to_string();
        log::info!(
            "step: window={}ms infer={:.2}s → {:?}",
            config.length_ms,
            took.as_secs_f32(),
            text_new
        );

        if text_new.is_empty() || is_hallucination(&text_new) {
            continue;
        }

        // Speech-end detection: did the speaker just pause? If so, lock
        // the current transcript as Final and reset for the next
        // utterance.
        let mut snap_for_vad = snapshot.clone();
        let speech_ended = vad::is_speech_end(
            &mut snap_for_vad,
            SAMPLE_RATE,
            500, // last 500 ms
            config.vad_thold,
            vad::DEFAULT_FREQ_THOLD_HZ,
        );

        if speech_ended {
            log::debug!("vad: speech-end detected; committing Final");
            emit_final(&mut transcript, &tx, text_new, &config);
            prev_text.clear();
            // Keep last 200 ms as priming for the next utterance.
            if ring.len() > keep_after_commit_samples {
                let drop_n = ring.len() - keep_after_commit_samples;
                ring.drain(..drop_n);
            }
            continue;
        }

        // Provisional — emit only the new suffix beyond `prev_text`.
        let suffix = strip_common_prefix(&text_new, &prev_text);
        if !suffix.is_empty() {
            let _ = tx.send(Segment {
                at: SystemTime::now(),
                text: suffix.to_string(),
                translation: None,
                kind: SegmentKind::Provisional,
            });
        }
        prev_text = text_new;
    }
    Ok(())
}

/// Translate (synchronous) and emit a Final Segment, also writing it to
/// the on-disk transcript. Centralised so both the VAD-end commit path
/// and the silent-window flush path share the same logic.
fn emit_final(
    transcript: &mut std::fs::File,
    tx: &Sender<Segment>,
    text: String,
    config: &SpeechTextConfig,
) {
    let captured_at = SystemTime::now();
    let translation = maybe_translate(&text, config);
    append_segment(transcript, captured_at, &text, translation.as_deref());
    let _ = tx.send(Segment {
        at: captured_at,
        text,
        translation,
        kind: SegmentKind::Final,
    });
}

fn maybe_translate(text: &str, config: &SpeechTextConfig) -> Option<String> {
    if !config.translate_enabled
        || config.translate_target.is_empty()
        || config.translate_target == config.language
    {
        return None;
    }
    match crate::translate::translate(text, &config.language, &config.translate_target) {
        Ok(en) if !en.trim().is_empty() => Some(en.trim().to_string()),
        Ok(_) => None,
        Err(e) => {
            log::warn!("translate failed: {}", e);
            Some("— translation unavailable —".to_string())
        }
    }
}

/// Return the suffix of `new` that is not a prefix of `prev`. UTF-8-safe.
/// If `new` does not extend `prev` (e.g. whisper revised earlier text),
/// returns the whole of `new` so the caller can show the revised line.
fn strip_common_prefix<'a>(new: &'a str, prev: &str) -> &'a str {
    if prev.is_empty() {
        return new;
    }
    if new.starts_with(prev) {
        let rest = &new[prev.len()..];
        return rest.trim_start();
    }
    new
}

/// Match a small set of whisper-on-silence hallucinations that the VAD
/// + `no_speech_thold` combo doesn't always catch. These all look like
/// "annotations" rather than real transcribed speech and are noise in a
/// German-language session.
fn is_hallucination(text: &str) -> bool {
    let t = text.trim();
    // Bare bracketed annotations: [Musik], [Music], [Applause], (Musik), * Musik *
    let stripped = t
        .trim_matches(|c: char| {
            c == '[' || c == ']' || c == '(' || c == ')' || c == '*' || c.is_whitespace()
        })
        .to_lowercase();
    matches!(
        stripped.as_str(),
        "musik"
            | "music"
            | "applaus"
            | "applause"
            | "geräusche"
            | "geräusch"
            | "lachen"
            | "schweigen"
            | "..."
            | ""
    )
}

/// Local broken-down time, no TZ offset. Sufficient for transcript timestamps;
/// good-enough for v1.
#[derive(Debug, Clone, Copy)]
struct LocalDateTime {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

fn local_datetime(when: SystemTime) -> LocalDateTime {
    let secs = when
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as libc::time_t)
        .unwrap_or(0);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&secs, &mut tm);
    }
    LocalDateTime {
        year: tm.tm_year + 1900,
        month: (tm.tm_mon + 1) as u32,
        day: tm.tm_mday as u32,
        hour: tm.tm_hour as u32,
        minute: tm.tm_min as u32,
        second: tm.tm_sec as u32,
    }
}

fn iso_local(when: SystemTime) -> String {
    let d = local_datetime(when);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        d.year, d.month, d.day, d.hour, d.minute, d.second
    )
}

fn hms_local(when: SystemTime) -> String {
    let d = local_datetime(when);
    format!("{:02}:{:02}:{:02}", d.hour, d.minute, d.second)
}

/// Open the transcript file in append mode, creating parent dirs. If the file
/// already has content (re-using a session name within the same day), prepend
/// a `---` separator so each launch is a clean front-matter block.
fn open_transcript(path: &Path, session: &SessionMeta, language: &str, model: &Path)
    -> Result<std::fs::File, String>
{
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create transcript dir {}: {}", parent.display(), e))?;
    }
    let preexisting = path.metadata().map(|m| m.len() > 0).unwrap_or(false);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("open transcript {}: {}", path.display(), e))?;
    if preexisting {
        writeln!(file, "\n---\n").ok();
    }
    let model_basename = model
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| model.display().to_string());
    writeln!(
        file,
        "---\ntitle: {}\nstarted_at: {}\nlanguage: {}\nmodel: {}\n---\n",
        session.name,
        iso_local(session.started_at),
        language,
        model_basename,
    )
    .map_err(|e| format!("write front-matter: {}", e))?;
    file.flush().ok();
    Ok(file)
}

fn append_segment(
    file: &mut std::fs::File,
    when: SystemTime,
    text: &str,
    translation: Option<&str>,
) {
    let _ = writeln!(file, "- **{}** — {}", hms_local(when), text);
    if let Some(en) = translation {
        let _ = writeln!(file, "  - _{}_", en);
    }
    let _ = file.flush();
}

/// Set SHUTDOWN to true from a signal handler. Called via `install_signal_handlers`.
extern "C" fn on_signal(_: libc::c_int) {
    crate::SHUTDOWN.store(true, Ordering::Relaxed);
}

/// Install SIGINT/SIGTERM handlers that flip the global SHUTDOWN flag.
/// Binaries without a GTK main loop (like the Phase 2 CLI) should call this once.
pub fn install_signal_handlers() {
    let handler = on_signal as *const () as libc::sighandler_t;
    unsafe {
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
}
