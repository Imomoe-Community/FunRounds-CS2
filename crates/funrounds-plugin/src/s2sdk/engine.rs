// Generated from s2sdk.pplugin (group: engine)

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

/// Returns the game's internal tick count.
///
/// # Returns - (int32):
/// The current tick count of the game.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetGameTickCount() -> i32 {
    unsafe { __s2sdk_GetGameTickCount.expect("GetGameTickCount function was not found")() }
}
pub type _GetGameTickCount = unsafe extern "C" fn() -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetGameTickCount: Option<_GetGameTickCount> = None;

/// Returns the maximum number of clients that can connect to the server.
///
/// # Returns - (int32):
/// The maximum client count, or -1 if global variables are not initialized.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetMaxClients() -> i32 {
    unsafe { __s2sdk_GetMaxClients.expect("GetMaxClients function was not found")() }
}
pub type _GetMaxClients = unsafe extern "C" fn() -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetMaxClients: Option<_GetMaxClients> = None;

/// Returns the game time based on the game tick.
///
/// # Returns - (float):
/// The current game time.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetGameTime() -> f32 {
    unsafe { __s2sdk_GetGameTime.expect("GetGameTime function was not found")() }
}
pub type _GetGameTime = unsafe extern "C" fn() -> f32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetGameTime: Option<_GetGameTime> = None;

/// Queries an interface from a specified module.
///
/// # Arguments
/// * `module` - (string): The name of the module to query the interface from.
/// * `name` - (string): The name of the interface to find.
///
/// # Returns - (ptr64):
/// A pointer to the queried interface.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn QueryInterface(module: &Str, name: &Str) -> usize {
    unsafe { __s2sdk_QueryInterface.expect("QueryInterface function was not found")(module, name) }
}
pub type _QueryInterface = unsafe extern "C" fn(&Str, &Str) -> usize;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_QueryInterface: Option<_QueryInterface> = None;
