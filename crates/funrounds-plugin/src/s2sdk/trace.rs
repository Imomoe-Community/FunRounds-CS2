// Generated from s2sdk.pplugin (group: trace)
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

/// Performs a line trace between two points.
///
/// # Arguments
/// * `startPos` - (vec3): Trace start position
/// * `endPos` - (vec3): Trace end position
/// * `mask` - (int32): Trace mask
/// * `ignoreHandle` - (int32): Entity handle to ignore during trace
/// * `outPos` - (vec3&): Output: position of impact
/// * `outFraction` - (double&): Output: fraction of trace completed
/// * `outHit` - (bool&): Output: whether a hit occurred
/// * `outEntHit` - (int32&): Output: handle of entity hit
/// * `outStartSolid` - (bool&): Output: whether trace started inside solid
///
/// # Returns - (bool):
/// True if trace hit something, false otherwise
#[track_caller]
#[allow(dead_code, non_snake_case, clippy::too_many_arguments)]
pub fn TraceLine(
    startPos: &Vec3,
    endPos: &Vec3,
    mask: i32,
    ignoreHandle: i32,
    outPos: &mut Vec3,
    outFraction: &mut f64,
    outHit: &mut bool,
    outEntHit: &mut i32,
    outStartSolid: &mut bool,
) -> bool {
    unsafe {
        __s2sdk_TraceLine.expect("TraceLine function was not found")(
            startPos,
            endPos,
            mask,
            ignoreHandle,
            outPos,
            outFraction,
            outHit,
            outEntHit,
            outStartSolid,
        )
    }
}
pub type _TraceLine = unsafe extern "C" fn(&Vec3, &Vec3, i32, i32, &mut Vec3, &mut f64, &mut bool, &mut i32, &mut bool) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_TraceLine: Option<_TraceLine> = None;
