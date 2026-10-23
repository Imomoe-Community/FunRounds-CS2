// Generated from funrounds_native.pplugin

#[allow(unused_imports)]
use super::aliases::*;
#[allow(unused_imports)]
use super::enums::*;
#[allow(unused_imports)]
use plugify::{Arr, Mat4x4, Str, Var, Vec2, Vec3, Vec4};

/// Synchronous pickup decision for exact player pawn and controller handles.
#[allow(unused)]
pub type PlayerCanAcquireCallback = unsafe extern "C" fn(i32, i32) -> i32;

/// Synchronous player damage decision with native damage type, exact weapon handle, and per-call rule scope.
#[allow(unused)]
pub type DamagePreCallback = unsafe extern "C" fn(i32, i32, i32, i32, i32, bool, f32, u32, &mut f32, &mut bool, i32, bool) -> i32;

/// Synchronous decision for every entity sharing the selected Touch implementation.
#[allow(unused)]
pub type EntityTouchPreCallback = unsafe extern "C" fn(i32, i32) -> i32;
