//! Platform-neutral audio API used by the Tauri commands.
//!
//! On Windows the real implementation talks to WASAPI / the MMDevice API.
//! On other platforms a mock backend is used so the UI can be developed anywhere.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flow {
    Output,
    Input,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub flow: Flow,
    pub is_default: bool,
    /// Endpoint master volume, 0.0 – 1.0.
    pub volume: f32,
    pub muted: bool,
    /// True when the default output is also being mirrored to this device.
    pub mirrored: bool,
    /// Extra delay applied to the mirrored audio (ms).
    pub delay_ms: u32,
}

#[cfg(windows)]
mod mirror;
#[cfg(windows)]
mod policy_config;
#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use self::{mirror::MirrorManager, win::*};

#[cfg(not(windows))]
mod mock;
#[cfg(not(windows))]
pub use self::mock::*;
