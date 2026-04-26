//! Capture supervisor — decides which backend to spawn (parec or pipewire),
//! resolves the target sink, and re-spawns the backend whenever the
//! PulseAudio / PipeWire default sink changes (Phase 7-prime).
//!
//! Design rationale: the user wants the speech-text feature to follow
//! whichever sink the audio control panel selects, including across
//! Bluetooth profile switches (A2DP ↔ HSP/HFP) and full device handoffs
//! (Jabra ↔ laptop speakers ↔ USB headset). The cleanest mechanism is to
//! subscribe to `pactl subscribe` events and treat any default-sink change
//! as "tear down + restart capture against the new sink." This is
//! agnostic to the backend, so both parec and pipewire benefit (although
//! pipewire's mainloop currently can't be cleanly stopped from outside —
//! see `pipewire.rs`).

use std::process::{Command, Stdio};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread;
use std::time::Duration;

use super::pipewire as pw_backend;
use super::parec as parec_backend;
use super::SpeechTextConfig;

/// Bounded queue depth for PCM chunks. At 16 kHz mono S16, 64 chunks ≈ a
/// few seconds of outstanding capture. Backpressure drops; never blocks
/// the producer.
const CHANNEL_DEPTH: usize = 64;

/// Spawn the capture supervisor. Returns a receiver of `Vec<i16>` PCM
/// chunks (S16_LE / 16 kHz / mono). The supervisor thread runs for the
/// process lifetime; SHUTDOWN is not yet observed here (Phase S6 will own
/// shutdown across the whole program).
pub fn start(config: &SpeechTextConfig) -> Result<Receiver<Vec<i16>>, String> {
    // Resolve initial target sink.
    let initial_sink = resolve_target_sink(&config.device_match)?;
    log::info!("capture supervisor: initial sink = {}", initial_sink);

    let (pcm_tx, pcm_rx) = sync_channel::<Vec<i16>>(CHANNEL_DEPTH);

    let device_match = config.device_match.clone();
    let backend_name = config.capture_backend.clone();

    thread::Builder::new()
        .name("capture-supervisor".into())
        .spawn(move || {
            run_supervisor(initial_sink, device_match, backend_name, pcm_tx);
        })
        .map_err(|e| format!("spawn capture supervisor: {}", e))?;

    Ok(pcm_rx)
}

/// Owned by either backend; both keep PCM samples flowing into the same
/// `pcm_tx`. Drop tears the backend down — the inner values are held only
/// for their Drop side-effects, hence `#[allow(dead_code)]`.
#[allow(dead_code)]
enum ActiveBackend {
    Parec(parec_backend::ParecBackend),
    PipeWire(pw_backend::PipeWireBackend),
}

fn run_supervisor(
    initial_sink: String,
    device_match: String,
    backend_name: String,
    pcm_tx: SyncSender<Vec<i16>>,
) {
    // Spawn the initial backend.
    let mut current_sink = initial_sink;
    let mut _backend = match spawn_backend(&backend_name, &current_sink, pcm_tx.clone()) {
        Ok(b) => Some(b),
        Err(e) => {
            log::error!("initial backend spawn failed: {}", e);
            None
        }
    };

    // Subscribe to pactl events and re-resolve on default-sink changes.
    let mut child = match Command::new("pactl")
        .arg("subscribe")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            log::error!(
                "pactl subscribe failed to spawn — auto-switch disabled: {}",
                e
            );
            // Park the thread so the backend stays alive.
            loop {
                thread::sleep(Duration::from_secs(3600));
            }
        }
    };

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            log::error!("pactl subscribe stdout missing — auto-switch disabled");
            loop {
                thread::sleep(Duration::from_secs(3600));
            }
        }
    };

    use std::io::{BufRead, BufReader};
    let reader = BufReader::new(stdout);

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        // We care about events that potentially mean "default sink changed":
        //   Event 'change' on server #N      (default sink changed)
        //   Event 'new'    on sink #N        (sink appeared, may be the new default)
        //   Event 'remove' on sink #N        (sink disappeared, default may have moved)
        // Anything else (sink-input events, source events, …) is noise.
        let interesting = line.contains("on server")
            || line.contains("on sink")
            || line.contains("on card");
        if !interesting {
            continue;
        }

        // Re-resolve. If unchanged, no work.
        let new_sink = match resolve_target_sink(&device_match) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("re-resolve failed ({}); keeping current backend", e);
                continue;
            }
        };
        if new_sink == current_sink {
            continue;
        }

        log::info!(
            "capture supervisor: sink changed {} -> {}; restarting backend",
            current_sink,
            new_sink
        );
        // Drop the old backend (kills parec / detaches pipewire). Then
        // spawn fresh against the new sink.
        _backend = None;
        match spawn_backend(&backend_name, &new_sink, pcm_tx.clone()) {
            Ok(b) => {
                _backend = Some(b);
                current_sink = new_sink;
            }
            Err(e) => {
                log::error!("backend re-spawn failed: {}", e);
                // Keep current_sink at the OLD value so the next event
                // tries again.
            }
        }
    }
    // pactl subscribe exited — usually means the user killed pulse/pw.
    log::warn!("pactl subscribe exited; auto-switch disabled");
    let _ = child.wait();
    // Park forever so the spawned backend keeps running until process exit.
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

fn spawn_backend(
    backend_name: &str,
    sink_name: &str,
    pcm_tx: SyncSender<Vec<i16>>,
) -> Result<ActiveBackend, String> {
    match backend_name {
        "pipewire" => pw_backend::start(sink_name, pcm_tx).map(ActiveBackend::PipeWire),
        "parec" | "" => parec_backend::start(sink_name, pcm_tx).map(ActiveBackend::Parec),
        other => Err(format!("unknown capture_backend {:?}", other)),
    }
}

/// Resolve the target sink name. If `device_match` is empty, returns
/// whatever `pactl get-default-sink` reports (so the speech-text process
/// follows the audio control panel's selection). Otherwise returns the
/// first sink whose description contains `device_match` (case-insensitive).
fn resolve_target_sink(device_match: &str) -> Result<String, String> {
    let needle = device_match.trim();
    if needle.is_empty() {
        return get_default_sink();
    }
    let needle_lc = needle.to_lowercase();
    let output = Command::new("pactl")
        .arg("list")
        .arg("sinks")
        .output()
        .map_err(|e| format!("pactl list sinks failed: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "pactl list sinks exit {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut current_name: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim_start();
        if let Some(rest) = line.strip_prefix("Name: ") {
            current_name = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("Description: ") {
            if rest.to_lowercase().contains(&needle_lc) {
                if let Some(name) = current_name.take() {
                    return Ok(name);
                }
            }
        }
    }
    Err(format!("no sink description matched {:?}", device_match))
}

fn get_default_sink() -> Result<String, String> {
    let output = Command::new("pactl")
        .arg("get-default-sink")
        .output()
        .map_err(|e| format!("pactl get-default-sink failed: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "pactl get-default-sink exit {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        Err("pactl get-default-sink returned empty".to_string())
    } else {
        Ok(name)
    }
}
