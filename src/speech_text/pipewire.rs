//! PipeWire client capture backend (opt-in via `capture_backend = "pipewire"`).
//!
//! Connects to the PipeWire daemon, opens a capture stream targeting
//! `<sink_name>.monitor` (PipeWire's idiomatic syntax for capturing the
//! monitor side of a sink), and pushes S16_LE / 16 kHz / mono samples to
//! the supplied `SyncSender`. Format is negotiated; PipeWire resamples and
//! downmixes the underlying source for us.
//!
//! ## Known limitation — HSP/HFP profile silence (PipeWire 1.0.5)
//!
//! On this host, native PipeWire client capture returns all-zero buffers
//! when a Bluetooth headset is in HSP/HFP profile (`headset-head-unit-msbc`).
//! The same monitor delivers real audio via the PulseAudio compat layer
//! (`parec`). For that reason, this backend is **not** the default — see
//! `parec.rs` for the always-works path.
//!
//! ## Lifecycle
//!
//! The backend runs its own thread until the process exits. There is no
//! cooperative shutdown — `Drop` is a no-op. Re-targeting on default-sink
//! change is not supported by this backend; if the user changes the default
//! sink while a session is active, the supervisor (in `capture.rs`) keeps
//! this backend running against the old sink. This is acceptable because
//! the pipewire backend is opt-in and chosen by users whose audio routing
//! is fixed.

use std::io::Cursor;
use std::sync::mpsc::SyncSender;
use std::thread;

use pipewire as pw;
use pw::spa::param::audio::{AudioFormat, AudioInfoRaw};
use pw::spa::param::ParamType;
use pw::spa::pod::{serialize::PodSerializer, Object, Pod, Value};
use pw::spa::utils::{Direction, SpaTypes};
use pw::stream::{StreamBox, StreamFlags};

/// Opaque handle. Drop is a no-op; the backend thread is detached for the
/// lifetime of the process.
pub struct PipeWireBackend {
    _private: (),
}

/// Spawn the PipeWire capture thread targeting `<sink_name>.monitor`.
/// Returns once the thread has been spawned; the actual stream-connect is
/// asynchronous and reported via `state_changed` log lines.
pub fn start(
    sink_name: &str,
    tx: SyncSender<Vec<i16>>,
) -> Result<PipeWireBackend, String> {
    let target = format!("{}.monitor", sink_name);

    thread::Builder::new()
        .name("pipewire-capture".into())
        .spawn(move || {
            if let Err(e) = run_loop(&target, tx) {
                log::error!("pipewire capture exited: {}", e);
            }
        })
        .map_err(|e| format!("spawn pipewire thread: {}", e))?;

    Ok(PipeWireBackend { _private: () })
}

fn run_loop(target_with_monitor: &str, tx: SyncSender<Vec<i16>>) -> Result<(), String> {
    pw::init();

    let mainloop =
        pw::main_loop::MainLoopRc::new(None).map_err(|e| format!("MainLoop: {}", e))?;
    let context = pw::context::ContextRc::new(&mainloop, None)
        .map_err(|e| format!("Context: {}", e))?;
    let core = context
        .connect_rc(None)
        .map_err(|e| format!("Core::connect: {}", e))?;

    // Avoid `media.role = "Communication"` — that pulls in echo-cancel /
    // noise-suppress filters that silence the loop-back path.
    let stream_props = pw::properties::properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Music",
        *pw::keys::TARGET_OBJECT => target_with_monitor.to_string(),
        *pw::keys::APP_NAME => "i3more-speech-text",
    };
    let stream = StreamBox::new(&core, "i3more-speech-text", stream_props)
        .map_err(|e| format!("StreamBox::new: {}", e))?;

    let _stream_listener = stream
        .add_local_listener_with_user_data(())
        .state_changed(|_stream, _ud, old, new| {
            log::info!("pipewire stream state: {:?} -> {:?}", old, new);
        })
        .process({
            let mut diag_window = 0u32;
            let mut diag_max: i16 = 0;
            move |stream, _ud| {
                let Some(mut buf) = stream.dequeue_buffer() else {
                    return;
                };
                let datas = buf.datas_mut();
                if datas.is_empty() {
                    return;
                }
                let chunk_size = datas[0].chunk().size() as usize;
                if chunk_size == 0 {
                    return;
                }
                let Some(bytes) = datas[0].data() else {
                    return;
                };
                let n_samples = chunk_size / 2;
                let mut samples = Vec::with_capacity(n_samples);
                let mut i = 0;
                while i + 1 < chunk_size && samples.len() < n_samples {
                    samples.push(i16::from_le_bytes([bytes[i], bytes[i + 1]]));
                    i += 2;
                }
                for s in &samples {
                    if s.unsigned_abs() as i16 > diag_max {
                        diag_max = s.unsigned_abs() as i16;
                    }
                }
                diag_window += samples.len() as u32;
                if diag_window >= 16_000 {
                    log::debug!("pw capture diag: peak |s| = {} over ~1s", diag_max);
                    diag_window = 0;
                    diag_max = 0;
                }
                let _ = tx.try_send(samples);
            }
        })
        .register()
        .map_err(|e| format!("register listener: {}", e))?;

    let mut info = AudioInfoRaw::new();
    info.set_format(AudioFormat::S16LE);
    info.set_rate(16_000);
    info.set_channels(1);
    let format_obj = Object {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };
    let format_bytes = PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(format_obj))
        .map_err(|e| format!("serialize format pod: {}", e))?
        .0
        .into_inner();
    let mut params = [Pod::from_bytes(&format_bytes).ok_or("Pod::from_bytes returned None")?];

    stream
        .connect(
            Direction::Input,
            None,
            StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|e| format!("Stream::connect: {}", e))?;

    log::info!("pipewire capture loop running; target={}", target_with_monitor);
    mainloop.run();
    log::info!("pipewire capture loop exited");
    Ok(())
}
