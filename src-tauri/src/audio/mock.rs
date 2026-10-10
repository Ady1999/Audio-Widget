//! In-memory fake backend so the widget UI can be developed on macOS / Linux.

use std::{collections::HashSet, sync::Mutex};

use super::{Device, Flow};

pub type Result<T> = std::result::Result<T, String>;

static DEVICES: Mutex<Vec<Device>> = Mutex::new(Vec::new());

fn with_devices<T>(f: impl FnOnce(&mut Vec<Device>) -> T) -> T {
    let mut devices = DEVICES.lock().unwrap();
    if devices.is_empty() {
        let mk = |id: &str, name: &str, flow, is_default| Device {
            id: id.into(),
            name: name.into(),
            flow,
            is_default,
            volume: 0.6,
            muted: false,
            mirrored: false,
            delay_ms: 0,
        };
        *devices = vec![
            mk("out-1", "Speakers (Realtek(R) Audio)", Flow::Output, true),
            mk("out-2", "Headphones (WH-1000XM5)", Flow::Output, false),
            mk(
                "out-3",
                "LG HDR 4K (NVIDIA High Definition Audio)",
                Flow::Output,
                false,
            ),
            mk(
                "in-1",
                "Microphone Array (Realtek(R) Audio)",
                Flow::Input,
                true,
            ),
            mk(
                "in-2",
                "Headset Microphone (WH-1000XM5)",
                Flow::Input,
                false,
            ),
        ];
    }
    f(&mut devices)
}

pub fn list_devices() -> Result<Vec<Device>> {
    Ok(with_devices(|d| d.clone()))
}

pub fn set_default(id: &str) -> Result<()> {
    with_devices(|devices| {
        let flow = devices
            .iter()
            .find(|d| d.id == id)
            .ok_or("unknown device")?
            .flow;
        for d in devices.iter_mut().filter(|d| d.flow == flow) {
            d.is_default = d.id == id;
        }
        Ok(())
    })
}

fn update(id: &str, f: impl FnOnce(&mut Device)) -> Result<()> {
    with_devices(|devices| {
        let d = devices
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or("unknown device")?;
        f(d);
        Ok(())
    })
}

pub fn set_volume(id: &str, volume: f32) -> Result<()> {
    update(id, |d| d.volume = volume.clamp(0.0, 1.0))
}

pub fn set_mute(id: &str, muted: bool) -> Result<()> {
    update(id, |d| d.muted = muted)
}

pub const MAX_DELAY_MS: u32 = 250;

#[derive(Default)]
pub struct MirrorManager {
    ids: HashSet<String>,
    pub delays: std::collections::HashMap<String, u32>,
}

impl MirrorManager {
    pub fn set_delay(&mut self, id: &str, ms: u32) {
        self.delays.insert(id.to_string(), ms.min(MAX_DELAY_MS));
    }

    pub fn active_ids(&mut self) -> Vec<String> {
        self.ids.iter().cloned().collect()
    }

    pub fn set(&mut self, id: &str, enabled: bool) -> Result<()> {
        if enabled {
            self.ids.insert(id.to_string());
        } else {
            self.ids.remove(id);
        }
        Ok(())
    }

    pub fn restart_all(&mut self, default_output: Option<&str>) -> Vec<String> {
        if let Some(id) = default_output {
            self.ids.remove(id);
        }
        Vec::new()
    }

    pub fn stop_all(&mut self) {
        self.ids.clear();
    }
}
