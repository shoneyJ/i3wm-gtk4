//! `parec` (PulseAudio compatibility) capture backend — the default.
//!
//! Spawns `parec --device=<sink>.monitor --format=s16le --rate=16000
//! --channels=1 --raw` as a subprocess and reads its stdout in a dedicated
//! thread, converting raw S16_LE bytes into `Vec<i16>` chunks pushed to a
//! bounded channel.
//!
//! Why `parec` despite the cleaner native-PipeWire alternative? On
//! PipeWire 1.0.5, the native client capture path returns silent buffers
//! when the target sink is in HSP/HFP profile (Bluetooth headset call
//! profile). The PulseAudio compatibility code path that backs `parec`
//! does not have this regression, so this backend works in every profile
//! we care about. See `pipewire.rs` for the alternative + caveat.
//!
//! Lifecycle: `Drop` on `ParecBackend` SIGKILLs the `parec` subprocess and
//! waits for it to exit. The reader thread then sees EOF on stdout and
//! exits naturally.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::SyncSender;
use std::thread::{self, JoinHandle};

/// Read buffer for parec stdout — small enough to keep latency low, large
/// enough to amortise read syscalls.
const READ_BUF_BYTES: usize = 8192;

/// Holds the parec subprocess and reader thread. Drop kills both.
pub struct ParecBackend {
    child: Child,
    reader: Option<JoinHandle<()>>,
}

/// Spawn `parec` against `<sink_name>.monitor` and start forwarding S16_LE
/// PCM chunks to `tx`.
pub fn start(sink_name: &str, tx: SyncSender<Vec<i16>>) -> Result<ParecBackend, String> {
    let device = format!("{}.monitor", sink_name);
    let mut child = Command::new("parec")
        .args([
            "--device",
            &device,
            "--format=s16le",
            "--rate=16000",
            "--channels=1",
            "--raw",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn parec: {}", e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "parec stdout not piped".to_string())?;

    let reader = thread::Builder::new()
        .name("parec-reader".into())
        .spawn(move || run_reader(stdout, tx))
        .map_err(|e| format!("spawn parec reader thread: {}", e))?;

    log::info!("parec capture started; device={}", device);
    Ok(ParecBackend {
        child,
        reader: Some(reader),
    })
}

impl Drop for ParecBackend {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(h) = self.reader.take() {
            // The reader sees EOF on stdout once parec is reaped and exits.
            let _ = h.join();
        }
        log::info!("parec capture stopped");
    }
}

fn run_reader<R: Read>(mut stdout: R, tx: SyncSender<Vec<i16>>) {
    let mut buf = [0u8; READ_BUF_BYTES];
    loop {
        let n = match stdout.read(&mut buf) {
            Ok(0) => {
                log::info!("parec stdout EOF; reader exiting");
                return;
            }
            Ok(n) => n,
            Err(e) => {
                log::error!("parec stdout read: {}", e);
                return;
            }
        };
        // Convert S16_LE bytes → Vec<i16>. We allocate per chunk; at 16 kHz
        // mono this is a few hundred bytes per call — negligible.
        let n_samples = n / 2;
        let mut samples = Vec::with_capacity(n_samples);
        let mut i = 0;
        while i + 1 < n {
            samples.push(i16::from_le_bytes([buf[i], buf[i + 1]]));
            i += 2;
        }
        // Drop on backpressure rather than block — same policy as the
        // pipewire backend's RT callback.
        let _ = tx.try_send(samples);
    }
}
