// Generated from s2sdk.pplugin (group: models)

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

/// Retrieves the attachment angles of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose attachment angles are to be retrieved.
/// * `attachmentIndex` - (int32): The attachment index.
///
/// # Returns - (vec3):
/// A vector representing the attachment angles (pitch, yaw, roll).
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEntityAttachmentAngles(entityHandle: i32, attachmentIndex: i32) -> Vec3 {
    unsafe { __s2sdk_GetEntityAttachmentAngles.expect("GetEntityAttachmentAngles function was not found")(entityHandle, attachmentIndex) }
}
pub type _GetEntityAttachmentAngles = unsafe extern "C" fn(i32, i32) -> Vec3;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEntityAttachmentAngles: Option<_GetEntityAttachmentAngles> = None;

/// Retrieves the origin vector of an entity's attachment.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose attachment origin is to be retrieved.
/// * `attachmentIndex` - (int32): The attachment index.
///
/// # Returns - (vec3):
/// A vector representing the origin of the attachment.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEntityAttachmentOrigin(entityHandle: i32, attachmentIndex: i32) -> Vec3 {
    unsafe { __s2sdk_GetEntityAttachmentOrigin.expect("GetEntityAttachmentOrigin function was not found")(entityHandle, attachmentIndex) }
}
pub type _GetEntityAttachmentOrigin = unsafe extern "C" fn(i32, i32) -> Vec3;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEntityAttachmentOrigin: Option<_GetEntityAttachmentOrigin> = None;

/// Retrieves an attachment index by name.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity.
/// * `attachmentName` - (string): The name of the attachment.
///
/// # Returns - (int32):
/// The attachment index, or -1 if not found.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn ScriptLookupAttachment(entityHandle: i32, attachmentName: &Str) -> i32 {
    unsafe { __s2sdk_ScriptLookupAttachment.expect("ScriptLookupAttachment function was not found")(entityHandle, attachmentName) }
}
pub type _ScriptLookupAttachment = unsafe extern "C" fn(i32, &Str) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_ScriptLookupAttachment: Option<_ScriptLookupAttachment> = None;
