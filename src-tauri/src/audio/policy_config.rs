//! The undocumented `IPolicyConfig` COM interface.
//!
//! This is what the Windows Sound control panel (and tools like SoundSwitch,
//! EarTrumpet, nircmd) use to change the default audio endpoint. It has been
//! stable since Windows 7 and still works on Windows 11.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::ffi::c_void;

use windows::Win32::Media::Audio::ERole;
use windows_core::{interface, IUnknown, IUnknown_Vtbl, GUID, HRESULT, PCWSTR};

pub const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
pub unsafe trait IPolicyConfig: IUnknown {
    pub fn GetMixFormat(&self, id: PCWSTR, format: *mut *mut c_void) -> HRESULT;
    pub fn GetDeviceFormat(&self, id: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT;
    pub fn ResetDeviceFormat(&self, id: PCWSTR) -> HRESULT;
    pub fn SetDeviceFormat(&self, id: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT;
    pub fn GetProcessingPeriod(
        &self,
        id: PCWSTR,
        default: i32,
        period: *mut i64,
        min: *mut i64,
    ) -> HRESULT;
    pub fn SetProcessingPeriod(&self, id: PCWSTR, period: *mut i64) -> HRESULT;
    pub fn GetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT;
    pub fn SetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT;
    pub fn GetPropertyValue(&self, id: PCWSTR, key: *const c_void, value: *mut c_void) -> HRESULT;
    pub fn SetPropertyValue(&self, id: PCWSTR, key: *const c_void, value: *mut c_void) -> HRESULT;
    pub fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT;
    pub fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT;
}
