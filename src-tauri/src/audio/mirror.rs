//! Multi-output: mirror whatever plays on the default output to extra devices.
//!
//! Windows can only have one default output, so for every extra device we run a
//! small pipeline: WASAPI *loopback* capture of the default output -> shared-mode
//! render on the target device. Windows does any sample-rate / channel conversion
//! for us (AUTOCONVERTPCM). Expect ~20-50 ms of latency on the mirrored devices.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use windows::Win32::{
    Media::Audio::{
        eConsole, eRender, IAudioCaptureClient, IAudioClient, IAudioRenderClient,
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
        AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
    },
    System::Com::{CoTaskMemFree, CLSCTX_ALL},
};

use super::win::{device_by_id, enumerator, err, ComGuard, Result};

/// 100 ms, in REFERENCE_TIME units (100 ns).
const BUFFER_DURATION: i64 = 1_000_000;
const POLL_INTERVAL: Duration = Duration::from_millis(4);

pub const MAX_DELAY_MS: u32 = 250;

struct Mirror {
    stop: Arc<AtomicBool>,
    delay_ms: Arc<AtomicU32>,
    handle: JoinHandle<()>,
}

#[derive(Default)]
pub struct MirrorManager {
    mirrors: HashMap<String, Mirror>,
    /// Extra delay per device (ms), kept even while a mirror is off.
    pub delays: HashMap<String, u32>,
}

impl MirrorManager {
    /// Ids of devices currently receiving mirrored audio.
    pub fn active_ids(&mut self) -> Vec<String> {
        self.mirrors.retain(|_, m| !m.handle.is_finished());
        self.mirrors.keys().cloned().collect()
    }

    pub fn set_delay(&mut self, id: &str, ms: u32) {
        let ms = ms.min(MAX_DELAY_MS);
        self.delays.insert(id.to_string(), ms);
        if let Some(m) = self.mirrors.get(id) {
            m.delay_ms.store(ms, Ordering::Relaxed);
        }
    }

    pub fn set(&mut self, id: &str, enabled: bool) -> Result<()> {
        if let Some(m) = self.mirrors.remove(id) {
            m.stop.store(true, Ordering::Relaxed);
            let _ = m.handle.join();
        }
        if !enabled {
            return Ok(());
        }

        let stop = Arc::new(AtomicBool::new(false));
        let delay_ms = Arc::new(AtomicU32::new(self.delays.get(id).copied().unwrap_or(0)));
        let thread_delay = delay_ms.clone();
        let (ready_tx, ready_rx) = mpsc::channel();
        let target = id.to_string();
        let thread_stop = stop.clone();
        let handle = thread::Builder::new()
            .name(format!("mirror-{id}"))
            .spawn(move || {
                if let Err(e) = run(&target, &thread_stop, &thread_delay, &ready_tx) {
                    let _ = ready_tx.send(Err(e.clone()));
                    eprintln!("mirror to {target} stopped: {e}");
                }
            })
            .map_err(|e| e.to_string())?;

        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => {
                self.mirrors.insert(
                    id.to_string(),
                    Mirror {
                        stop,
                        delay_ms,
                        handle,
                    },
                );
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                Err("Timed out starting audio mirror".into())
            }
        }
    }

    /// Restart every mirror, e.g. after the default output changed (the loopback
    /// source is bound to the device that was default when the mirror started).
    pub fn restart_all(&mut self, default_output: Option<&str>) -> Vec<String> {
        let ids = self.active_ids();
        let mut failed = Vec::new();
        for id in ids {
            let keep = default_output != Some(id.as_str());
            if self.set(&id, keep).is_err() {
                failed.push(id);
            }
        }
        failed
    }

    pub fn stop_all(&mut self) {
        for (_, m) in self.mirrors.drain() {
            m.stop.store(true, Ordering::Relaxed);
            let _ = m.handle.join();
        }
    }
}

impl Drop for MirrorManager {
    fn drop(&mut self) {
        self.stop_all();
    }
}

fn run(
    target_id: &str,
    stop: &AtomicBool,
    delay_ms: &AtomicU32,
    ready: &mpsc::Sender<Result<()>>,
) -> Result<()> {
    let _com = ComGuard::new();
    let en = enumerator()?;

    unsafe {
        let source = en.GetDefaultAudioEndpoint(eRender, eConsole).map_err(err)?;
        let target = device_by_id(&en, target_id)?;

        let capture_client: IAudioClient = source.Activate(CLSCTX_ALL, None).map_err(err)?;
        let format = capture_client.GetMixFormat().map_err(err)?;
        let block_align = (*format).nBlockAlign as usize;
        let bytes_per_sec = (*format).nAvgBytesPerSec as usize;

        let init = (|| -> Result<(IAudioCaptureClient, IAudioClient, IAudioRenderClient)> {
            capture_client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK,
                    BUFFER_DURATION,
                    0,
                    format,
                    None,
                )
                .map_err(err)?;
            let capture: IAudioCaptureClient = capture_client.GetService().map_err(err)?;

            let render_client: IAudioClient = target.Activate(CLSCTX_ALL, None).map_err(err)?;
            render_client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                    BUFFER_DURATION,
                    0,
                    format,
                    None,
                )
                .map_err(err)?;
            let render: IAudioRenderClient = render_client.GetService().map_err(err)?;
            Ok((capture, render_client, render))
        })();
        CoTaskMemFree(Some(format as _));
        let (capture, render_client, render) = init?;

        let render_frames = render_client.GetBufferSize().map_err(err)? as usize;
        capture_client.Start().map_err(err)?;
        render_client.Start().map_err(err)?;
        let _ = ready.send(Ok(()));

        let ms_to_bytes = |ms: usize| (bytes_per_sec * ms / 1000) / block_align * block_align;
        // Beyond the user's delay keep at most ~60 ms queued; older audio is dropped so
        // the mirrored device never drifts noticeably behind the primary one.
        let slack = ms_to_bytes(60);
        let mut queue: VecDeque<u8> = VecDeque::with_capacity(bytes_per_sec / 2);
        let (mut last_packet, mut last_tick) = (Instant::now(), Instant::now());

        let result = (|| -> Result<()> {
            while !stop.load(Ordering::Relaxed) {
                let delay = ms_to_bytes(delay_ms.load(Ordering::Relaxed) as usize);
                let tick = last_tick.elapsed();
                last_tick = Instant::now();

                while capture.GetNextPacketSize().map_err(err)? > 0 {
                    last_packet = Instant::now();
                    let mut data = std::ptr::null_mut();
                    let mut frames = 0u32;
                    let mut flags = 0u32;
                    capture
                        .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                        .map_err(err)?;
                    let len = frames as usize * block_align;
                    if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                        queue.extend(std::iter::repeat_n(0u8, len));
                    } else {
                        queue.extend(std::slice::from_raw_parts(data, len));
                    }
                    capture.ReleaseBuffer(frames).map_err(err)?;
                }

                // Loopback sends nothing while the PC is silent. Feed silence then, so the
                // held-back tail still plays on time instead of when sound resumes.
                if delay > 0 && last_packet.elapsed() > Duration::from_millis(30) {
                    let len = ms_to_bytes(tick.as_millis() as usize);
                    queue.extend(std::iter::repeat_n(0u8, len));
                }

                if queue.len() > delay + slack {
                    let excess = queue.len() - delay - slack / 2;
                    queue.drain(..excess - excess % block_align);
                }

                let padding = render_client.GetCurrentPadding().map_err(err)? as usize;
                let ready = queue.len().saturating_sub(delay);
                let frames = (render_frames - padding).min(ready / block_align);
                if frames > 0 {
                    let buffer = render.GetBuffer(frames as u32).map_err(err)?;
                    let out = std::slice::from_raw_parts_mut(buffer, frames * block_align);
                    for (dst, src) in out.iter_mut().zip(queue.drain(..frames * block_align)) {
                        *dst = src;
                    }
                    render.ReleaseBuffer(frames as u32, 0).map_err(err)?;
                }

                thread::sleep(POLL_INTERVAL);
            }
            Ok(())
        })();

        let _ = capture_client.Stop();
        let _ = render_client.Stop();
        result
    }
}
