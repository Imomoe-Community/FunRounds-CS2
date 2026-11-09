// Generated from funrounds_native.pplugin (group: wallhang)
// SPDX-License-Identifier: AGPL-3.0-or-later

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

/// On the game thread, checks the exact paired CS2 26445667 movement-trace ABI, server image identity, and physics-query receiver. Unsupported or mismatched server builds return false.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HasPlayerMovementHullSupport() -> bool {
    unsafe { __funrounds_native_HasPlayerMovementHullSupport.expect("HasPlayerMovementHullSupport function was not found")() }
}
pub type _HasPlayerMovementHullSupport = unsafe extern "C" fn() -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HasPlayerMovementHullSupport: Option<_HasPlayerMovementHullSupport> = None;

/// On the game thread, traces the supplied real local hull using the pinned player-movement filter while ignoring one exact full player-pawn handle. Returns true only after finite trace outputs are validated; a successful miss returns true with outHit=false and outEntityHandle=-1. Failure returns false with deterministic miss outputs. It never passes engine pointers across this provider boundary.
///
/// # Arguments
/// * `pawnHandle` - (int32)
/// * `start` - (vec3)
/// * `end` - (vec3)
/// * `hullMins` - (vec3)
/// * `hullMaxs` - (vec3)
/// * `outPosition` - (vec3&)
/// * `outFraction` - (float&)
/// * `outHit` - (bool&)
/// * `outEntityHandle` - (int32&)
/// * `outStartSolid` - (bool&)
/// * `outNormal` - (vec3&)
#[track_caller]
#[allow(dead_code, non_snake_case, clippy::too_many_arguments)]
pub fn TracePlayerMovementHull(
    pawnHandle: i32,
    start: &Vec3,
    end: &Vec3,
    hullMins: &Vec3,
    hullMaxs: &Vec3,
    outPosition: &mut Vec3,
    outFraction: &mut f32,
    outHit: &mut bool,
    outEntityHandle: &mut i32,
    outStartSolid: &mut bool,
    outNormal: &mut Vec3,
) -> bool {
    unsafe {
        __funrounds_native_TracePlayerMovementHull.expect("TracePlayerMovementHull function was not found")(
            pawnHandle,
            start,
            end,
            hullMins,
            hullMaxs,
            outPosition,
            outFraction,
            outHit,
            outEntityHandle,
            outStartSolid,
            outNormal,
        )
    }
}
pub type _TracePlayerMovementHull =
    unsafe extern "C" fn(i32, &Vec3, &Vec3, &Vec3, &Vec3, &mut Vec3, &mut f32, &mut bool, &mut i32, &mut bool, &mut Vec3) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_TracePlayerMovementHull: Option<_TracePlayerMovementHull> = None;
