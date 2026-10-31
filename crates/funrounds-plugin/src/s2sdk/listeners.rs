// Generated from s2sdk.pplugin (group: listeners)
// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(non_camel_case_types)]

#[allow(unused_imports)]
use super::aliases::*;
#[allow(unused_imports)]
use super::delegates::*;
#[allow(unused_imports)]
use super::enums::*;
#[allow(unused_imports)]
use plugify::{Arr, Mat4x4, Str, Var, Vec2, Vec3, Vec4, scope};
#[allow(unused_imports)]
use std::sync::RwLock;

/// Register callback to event.
///
/// # Arguments
/// * `callback` - (function): Function callback.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn OnBuildGameSessionManifest_Register(callback: OnBuildGameSessionManifestCallback) {
    unsafe { __s2sdk_OnBuildGameSessionManifest_Register.expect("OnBuildGameSessionManifest_Register function was not found")(callback) }
}
pub type _OnBuildGameSessionManifest_Register = unsafe extern "C" fn(OnBuildGameSessionManifestCallback);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_OnBuildGameSessionManifest_Register: Option<_OnBuildGameSessionManifest_Register> = None;

/// Unregister callback to event.
///
/// # Arguments
/// * `callback` - (function): Function callback.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn OnBuildGameSessionManifest_Unregister(callback: OnBuildGameSessionManifestCallback) {
    unsafe { __s2sdk_OnBuildGameSessionManifest_Unregister.expect("OnBuildGameSessionManifest_Unregister function was not found")(callback) }
}
pub type _OnBuildGameSessionManifest_Unregister = unsafe extern "C" fn(OnBuildGameSessionManifestCallback);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_OnBuildGameSessionManifest_Unregister: Option<_OnBuildGameSessionManifest_Unregister> = None;

/// Register callback to event.
///
/// # Arguments
/// * `callback` - (function): Function callback.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn OnMapEnd_Register(callback: OnMapEndCallback) {
    unsafe { __s2sdk_OnMapEnd_Register.expect("OnMapEnd_Register function was not found")(callback) }
}
pub type _OnMapEnd_Register = unsafe extern "C" fn(OnMapEndCallback);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_OnMapEnd_Register: Option<_OnMapEnd_Register> = None;

/// Unregister callback to event.
///
/// # Arguments
/// * `callback` - (function): Function callback.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn OnMapEnd_Unregister(callback: OnMapEndCallback) {
    unsafe { __s2sdk_OnMapEnd_Unregister.expect("OnMapEnd_Unregister function was not found")(callback) }
}
pub type _OnMapEnd_Unregister = unsafe extern "C" fn(OnMapEndCallback);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_OnMapEnd_Unregister: Option<_OnMapEnd_Unregister> = None;
