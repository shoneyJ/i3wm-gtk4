//! Phase-2 throwaway CLI: prints each transcribed segment to stdout.
//! Replaced by a GTK4 binary in Phase 3.

use std::sync::mpsc;
use std::time::{Duration, UNIX_EPOCH};

use i3more::speech_text::{
    install_signal_handlers, load_config, SegmentKind, SessionMeta, SpeechSession,
};

fn main() {
    i3more::init_logging("speech-text");
    install_signal_handlers();
    log::info!("i3more-speech-text starting");

    let config = load_config();
    println!(
        "config: model={} lang={} device_match={:?} chunk={}s translate={}{}",
        config.model_path.display(),
        config.language,
        config.device_match,
        config.chunk_seconds,
        config.translate_enabled,
        if config.translate_enabled {
            format!(" → {}", config.translate_target)
        } else {
            String::new()
        }
    );

    // Session name precedence: CLI arg `--session=<name>` > env I3MORE_STT_SESSION > default.
    let session_name = parse_session_arg().or_else(|| std::env::var("I3MORE_STT_SESSION").ok());
    let meta = SessionMeta::new(session_name);
    println!(
        "session: {} → {}",
        meta.name,
        meta.transcript_path().display()
    );

    let (tx, rx) = mpsc::channel();

    let session = match SpeechSession::start(config, meta, tx) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to start session: {}", e);
            std::process::exit(1);
        }
    };
    println!("session started — Ctrl-C to stop");

    loop {
        if i3more::shutdown_requested() {
            println!("\nshutting down");
            break;
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(seg) => {
                let secs = seg
                    .at
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let hms = format_hms(secs);
                match seg.kind {
                    SegmentKind::Provisional => {
                        // Mid-utterance — render dimmed without timestamp
                        // and without writing a newline so subsequent
                        // updates flow naturally. ANSI dim = ESC[2m.
                        println!("\x1b[2m  …{}\x1b[0m", seg.text);
                    }
                    SegmentKind::Final => {
                        println!("[{}]  {}", hms, seg.text);
                        if let Some(en) = seg.translation.as_deref() {
                            println!("          ↳ {}", en);
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                println!("\nsession channel closed");
                break;
            }
        }
    }

    drop(session);
    log::info!("i3more-speech-text exited");
}

/// Local wall-clock HH:MM:SS from UNIX seconds (no TZ offset — good enough for a
/// phase-2 CLI; the GTK UI will use proper formatting later).
fn format_hms(unix_secs: u64) -> String {
    let seconds_of_day = unix_secs % 86_400;
    let h = seconds_of_day / 3600;
    let m = (seconds_of_day % 3600) / 60;
    let s = seconds_of_day % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

/// Parse `--session=<name>` from argv. Returns the value, or None if absent.
fn parse_session_arg() -> Option<String> {
    for arg in std::env::args().skip(1) {
        if let Some(name) = arg.strip_prefix("--session=") {
            return Some(name.to_string());
        }
    }
    None
}
