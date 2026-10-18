// Generated from s2sdk.pplugin (group: schema)

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

/// Get the offset of a member in a given schema class.
///
/// # Arguments
/// * `className` - (string): The name of the class.
/// * `memberName` - (string): The name of the member whose offset is to be retrieved.
///
/// # Returns - (int32):
/// The offset of the member in the class, or -1 if the offset is not found.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetSchemaOffset(className: &Str, memberName: &Str) -> i32 {
    unsafe { __s2sdk_GetSchemaOffset.expect("GetSchemaOffset function was not found")(className, memberName) }
}
pub type _GetSchemaOffset = unsafe extern "C" fn(&Str, &Str) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetSchemaOffset: Option<_GetSchemaOffset> = None;

/// Retrieves an integer value from an entity's schema.
///
/// # Arguments
/// * `entity` - (ptr64): Pointer to the instance of the class where the value is to be set.
/// * `className` - (string): The name of the class.
/// * `memberName` - (string): The name of the schema member.
/// * `element` - (int32): Element # (starting from 0) if schema is an array.
///
/// # Returns - (int64):
/// An integer value at the given schema offset.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEntSchema2(entity: usize, className: &Str, memberName: &Str, element: i32) -> i64 {
    unsafe { __s2sdk_GetEntSchema2.expect("GetEntSchema2 function was not found")(entity, className, memberName, element) }
}
pub type _GetEntSchema2 = unsafe extern "C" fn(usize, &Str, &Str, i32) -> i64;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEntSchema2: Option<_GetEntSchema2> = None;

/// Sets an integer value in an entity's schema.
///
/// # Arguments
/// * `entity` - (ptr64): Pointer to the instance of the class where the value is to be set.
/// * `className` - (string): The name of the class.
/// * `memberName` - (string): The name of the schema member.
/// * `value` - (int64): The integer value to set.
/// * `changeState` - (bool): If true, change will be sent over the network.
/// * `element` - (int32): Element # (starting from 0) if schema is an array.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetEntSchema2(entity: usize, className: &Str, memberName: &Str, value: i64, changeState: bool, element: i32) {
    unsafe { __s2sdk_SetEntSchema2.expect("SetEntSchema2 function was not found")(entity, className, memberName, value, changeState, element) }
}
pub type _SetEntSchema2 = unsafe extern "C" fn(usize, &Str, &Str, i64, bool, i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetEntSchema2: Option<_SetEntSchema2> = None;

/// Retrieves a float value from an entity's schema.
///
/// # Arguments
/// * `entity` - (ptr64): Pointer to the instance of the class where the value is to be set.
/// * `className` - (string): The name of the class.
/// * `memberName` - (string): The name of the schema member.
/// * `element` - (int32): Element # (starting from 0) if schema is an array.
///
/// # Returns - (double):
/// A float value at the given schema offset.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEntSchemaFloat2(entity: usize, className: &Str, memberName: &Str, element: i32) -> f64 {
    unsafe { __s2sdk_GetEntSchemaFloat2.expect("GetEntSchemaFloat2 function was not found")(entity, className, memberName, element) }
}
pub type _GetEntSchemaFloat2 = unsafe extern "C" fn(usize, &Str, &Str, i32) -> f64;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEntSchemaFloat2: Option<_GetEntSchemaFloat2> = None;

/// Sets a float value in an entity's schema.
///
/// # Arguments
/// * `entity` - (ptr64): Pointer to the instance of the class where the value is to be set.
/// * `className` - (string): The name of the class.
/// * `memberName` - (string): The name of the schema member.
/// * `value` - (double): The float value to set.
/// * `changeState` - (bool): If true, change will be sent over the network.
/// * `element` - (int32): Element # (starting from 0) if schema is an array.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetEntSchemaFloat2(entity: usize, className: &Str, memberName: &Str, value: f64, changeState: bool, element: i32) {
    unsafe {
        __s2sdk_SetEntSchemaFloat2.expect("SetEntSchemaFloat2 function was not found")(entity, className, memberName, value, changeState, element)
    }
}
pub type _SetEntSchemaFloat2 = unsafe extern "C" fn(usize, &Str, &Str, f64, bool, i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetEntSchemaFloat2: Option<_SetEntSchemaFloat2> = None;

/// Retrieves a vector value from an entity's schema.
///
/// # Arguments
/// * `entity` - (ptr64): Pointer to the instance of the class where the value is to be set.
/// * `className` - (string): The name of the class.
/// * `memberName` - (string): The name of the schema member.
/// * `element` - (int32): Element # (starting from 0) if schema is an array.
///
/// # Returns - (vec3):
/// A vector value at the given schema offset.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEntSchemaVector3D2(entity: usize, className: &Str, memberName: &Str, element: i32) -> Vec3 {
    unsafe { __s2sdk_GetEntSchemaVector3D2.expect("GetEntSchemaVector3D2 function was not found")(entity, className, memberName, element) }
}
pub type _GetEntSchemaVector3D2 = unsafe extern "C" fn(usize, &Str, &Str, i32) -> Vec3;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEntSchemaVector3D2: Option<_GetEntSchemaVector3D2> = None;

/// Sets a vector value in an entity's schema.
///
/// # Arguments
/// * `entity` - (ptr64): Pointer to the instance of the class where the value is to be set.
/// * `className` - (string): The name of the class.
/// * `memberName` - (string): The name of the schema member.
/// * `value` - (vec3): The vector value to set.
/// * `changeState` - (bool): If true, change will be sent over the network.
/// * `element` - (int32): Element # (starting from 0) if schema is an array.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetEntSchemaVector3D2(entity: usize, className: &Str, memberName: &Str, value: &Vec3, changeState: bool, element: i32) {
    unsafe {
        __s2sdk_SetEntSchemaVector3D2.expect("SetEntSchemaVector3D2 function was not found")(
            entity,
            className,
            memberName,
            value,
            changeState,
            element,
        )
    }
}
pub type _SetEntSchemaVector3D2 = unsafe extern "C" fn(usize, &Str, &Str, &Vec3, bool, i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetEntSchemaVector3D2: Option<_SetEntSchemaVector3D2> = None;

/// Retrieves an entity handle from an entity's schema.
///
/// # Arguments
/// * `entity` - (ptr64): Pointer to the instance of the class where the value is to be set.
/// * `className` - (string): The name of the class.
/// * `memberName` - (string): The name of the schema member.
/// * `element` - (int32): Element # (starting from 0) if schema is an array.
///
/// # Returns - (int32):
/// A string value at the given schema offset.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEntSchemaEnt2(entity: usize, className: &Str, memberName: &Str, element: i32) -> i32 {
    unsafe { __s2sdk_GetEntSchemaEnt2.expect("GetEntSchemaEnt2 function was not found")(entity, className, memberName, element) }
}
pub type _GetEntSchemaEnt2 = unsafe extern "C" fn(usize, &Str, &Str, i32) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEntSchemaEnt2: Option<_GetEntSchemaEnt2> = None;

/// Sets an entity handle in an entity's schema.
///
/// # Arguments
/// * `entity` - (ptr64): Pointer to the instance of the class where the value is to be set.
/// * `className` - (string): The name of the class.
/// * `memberName` - (string): The name of the schema member.
/// * `value` - (int32): The entity handle to set.
/// * `changeState` - (bool): If true, change will be sent over the network.
/// * `element` - (int32): Element # (starting from 0) if schema is an array.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetEntSchemaEnt2(entity: usize, className: &Str, memberName: &Str, value: i32, changeState: bool, element: i32) {
    unsafe { __s2sdk_SetEntSchemaEnt2.expect("SetEntSchemaEnt2 function was not found")(entity, className, memberName, value, changeState, element) }
}
pub type _SetEntSchemaEnt2 = unsafe extern "C" fn(usize, &Str, &Str, i32, bool, i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetEntSchemaEnt2: Option<_SetEntSchemaEnt2> = None;
