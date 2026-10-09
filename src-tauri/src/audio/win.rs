//! Windows backend: device enumeration, default switching and volume via WASAPI.

use windows::Win32::{
    Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
    Media::Audio::{
        eCapture, eCommunications, eConsole, eMultimedia, eRender, EDataFlow,
        Endpoints::IAudioEndpointVolume, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
        DEVICE_STATE_ACTIVE,
    },
    System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_MULTITHREADED, STGM_READ,
    },
};
use windows_core::{HSTRING, PCWSTR};

use super::{policy_config::*, Device, Flow};

pub type Result<T> = std::result::Result<T, String>;

pub(crate) fn err(e: windows_core::Error) -> String {
    format!("{} (0x{:08X})", e.message(), e.code().0)
}

/// Initializes COM (MTA) for the current thread for as long as it lives.
pub(crate) struct ComGuard(bool);

impl ComGuard {
    pub fn new() -> Self {
        // RPC_E_CHANGED_MODE means the thread is already an STA, which is fine for us.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        ComGuard(hr.is_ok())
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

pub(crate) fn enumerator() -> Result<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(err)
}

pub(crate) fn device_by_id(en: &IMMDeviceEnumerator, id: &str) -> Result<IMMDevice> {
    unsafe { en.GetDevice(&HSTRING::from(id)) }.map_err(err)
}

fn device_id(device: &IMMDevice) -> Result<String> {
    unsafe {
        let pwstr = device.GetId().map_err(err)?;
        let id = pwstr.to_string().map_err(|e| e.to_string());
        CoTaskMemFree(Some(pwstr.0 as _));
        id
    }
}

fn friendly_name(device: &IMMDevice) -> Result<String> {
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).map_err(err)?;
        let value = store.GetValue(&PKEY_Device_FriendlyName).map_err(err)?;
        Ok(value.to_string())
    }
}

fn endpoint_volume(device: &IMMDevice) -> Result<IAudioEndpointVolume> {
    unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(err)
}

fn collect(
    en: &IMMDeviceEnumerator,
    data_flow: EDataFlow,
    flow: Flow,
    out: &mut Vec<Device>,
) -> Result<()> {
    let default_id = unsafe { en.GetDefaultAudioEndpoint(data_flow, eConsole) }
        .ok()
        .and_then(|d| device_id(&d).ok());

    let collection =
        unsafe { en.EnumAudioEndpoints(data_flow, DEVICE_STATE_ACTIVE) }.map_err(err)?;
    let count = unsafe { collection.GetCount() }.map_err(err)?;

    for i in 0..count {
        let Ok(device) = (unsafe { collection.Item(i) }) else {
            continue;
        };
        let Ok(id) = device_id(&device) else { continue };
        let name = friendly_name(&device).unwrap_or_else(|_| "Unknown device".into());
        let (volume, muted) = endpoint_volume(&device)
            .and_then(|v| unsafe {
                Ok((
                    v.GetMasterVolumeLevelScalar().map_err(err)?,
                    v.GetMute().map_err(err)?.as_bool(),
                ))
            })
            .unwrap_or((0.0, false));

        out.push(Device {
            is_default: default_id.as_deref() == Some(id.as_str()),
            id,
            name,
            flow,
            volume,
            muted,
            mirrored: false,
        });
    }
    Ok(())
}

pub fn list_devices() -> Result<Vec<Device>> {
    let _com = ComGuard::new();
    let en = enumerator()?;
    let mut devices = Vec::new();
    collect(&en, eRender, Flow::Output, &mut devices)?;
    collect(&en, eCapture, Flow::Input, &mut devices)?;
    Ok(devices)
}

/// Makes `id` the default device for all roles (console, multimedia, communications).
pub fn set_default(id: &str) -> Result<()> {
    let _com = ComGuard::new();
    let policy: IPolicyConfig =
        unsafe { CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL) }.map_err(err)?;
    let wide = HSTRING::from(id);
    for role in [eConsole, eMultimedia, eCommunications] {
        unsafe { policy.SetDefaultEndpoint(PCWSTR(wide.as_ptr()), role) }
            .ok()
            .map_err(err)?;
    }
    Ok(())
}

pub fn set_volume(id: &str, volume: f32) -> Result<()> {
    let _com = ComGuard::new();
    let device = device_by_id(&enumerator()?, id)?;
    let v = endpoint_volume(&device)?;
    unsafe { v.SetMasterVolumeLevelScalar(volume.clamp(0.0, 1.0), std::ptr::null()) }.map_err(err)
}

pub fn set_mute(id: &str, muted: bool) -> Result<()> {
    let _com = ComGuard::new();
    let device = device_by_id(&enumerator()?, id)?;
    let v = endpoint_volume(&device)?;
    unsafe { v.SetMute(muted, std::ptr::null()) }.map_err(err)
}
