// Generated from s2sdk.pplugin (group: entities)

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

/// Converts an entity pointer into an entity handle.
///
/// # Arguments
/// * `entity` - (ptr64): A pointer to the entity to convert.
///
/// # Returns - (int32):
/// The entity handle as an integer, or INVALID_EHANDLE_INDEX if the entity is nullptr.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn EntPointerToEntHandle(entity: usize) -> i32 {
    unsafe { __s2sdk_EntPointerToEntHandle.expect("EntPointerToEntHandle function was not found")(entity) }
}
pub type _EntPointerToEntHandle = unsafe extern "C" fn(usize) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_EntPointerToEntHandle: Option<_EntPointerToEntHandle> = None;

/// Retrieves the entity pointer from an entity handle.
///
/// # Arguments
/// * `entityHandle` - (int32): The entity handle to convert.
///
/// # Returns - (ptr64):
/// A pointer to the entity instance, or nullptr if the handle is invalid.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn EntHandleToEntPointer(entityHandle: i32) -> usize {
    unsafe { __s2sdk_EntHandleToEntPointer.expect("EntHandleToEntPointer function was not found")(entityHandle) }
}
pub type _EntHandleToEntPointer = unsafe extern "C" fn(i32) -> usize;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_EntHandleToEntPointer: Option<_EntHandleToEntPointer> = None;

/// Checks if the provided entity handle is valid.
///
/// # Arguments
/// * `entityHandle` - (int32): The entity handle to check.
///
/// # Returns - (bool):
/// True if the entity handle is valid, false otherwise.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn IsValidEntHandle(entityHandle: i32) -> bool {
    unsafe { __s2sdk_IsValidEntHandle.expect("IsValidEntHandle function was not found")(entityHandle) }
}
pub type _IsValidEntHandle = unsafe extern "C" fn(i32) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_IsValidEntHandle: Option<_IsValidEntHandle> = None;

/// Retrieves the class name of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose class name is to be retrieved.
///
/// # Returns - (string):
/// A string where the class name will be stored.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEntityClassname(entityHandle: i32) -> Str {
    unsafe { __s2sdk_GetEntityClassname.expect("GetEntityClassname function was not found")(entityHandle) }
}
pub type _GetEntityClassname = unsafe extern "C" fn(i32) -> Str;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEntityClassname: Option<_GetEntityClassname> = None;

/// Retrieves the owner of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose owner is to be retrieved.
///
/// # Returns - (int32):
/// The handle of the owner entity, or INVALID_EHANDLE_INDEX if the entity is invalid.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEntityOwner(entityHandle: i32) -> i32 {
    unsafe { __s2sdk_GetEntityOwner.expect("GetEntityOwner function was not found")(entityHandle) }
}
pub type _GetEntityOwner = unsafe extern "C" fn(i32) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEntityOwner: Option<_GetEntityOwner> = None;

/// Marks an entity for deletion.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity to be deleted.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn RemoveEntity(entityHandle: i32) {
    unsafe { __s2sdk_RemoveEntity.expect("RemoveEntity function was not found")(entityHandle) }
}
pub type _RemoveEntity = unsafe extern "C" fn(i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_RemoveEntity: Option<_RemoveEntity> = None;

/// Converts an entity index into an entity handle.
///
/// # Arguments
/// * `entityIndex` - (int32): The index of the entity to convert.
///
/// # Returns - (int32):
/// The entity handle as an integer, or -1 if the entity index is invalid.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn EntIndexToEntHandle(entityIndex: i32) -> i32 {
    unsafe { __s2sdk_EntIndexToEntHandle.expect("EntIndexToEntHandle function was not found")(entityIndex) }
}
pub type _EntIndexToEntHandle = unsafe extern "C" fn(i32) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_EntIndexToEntHandle: Option<_EntIndexToEntHandle> = None;

/// Retrieves the pointer to the first active entity.
///
/// # Returns - (int32):
/// A handle to the first active entity.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetFirstActiveEntity() -> i32 {
    unsafe { __s2sdk_GetFirstActiveEntity.expect("GetFirstActiveEntity function was not found")() }
}
pub type _GetFirstActiveEntity = unsafe extern "C" fn() -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetFirstActiveEntity: Option<_GetFirstActiveEntity> = None;

/// Retrieves the next active entity.
///
/// # Arguments
/// * `entityHandle` - (int32)
///
/// # Returns - (int32):
/// Handle to the next entity.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetNextActiveEntity(entityHandle: i32) -> i32 {
    unsafe { __s2sdk_GetNextActiveEntity.expect("GetNextActiveEntity function was not found")(entityHandle) }
}
pub type _GetNextActiveEntity = unsafe extern "C" fn(i32) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetNextActiveEntity: Option<_GetNextActiveEntity> = None;

/// Finds an entity by classname with iteration.
///
/// # Arguments
/// * `startFrom` - (int32): The handle of the entity to start from, or INVALID_EHANDLE_INDEX to start fresh.
/// * `classname` - (string): The class name to search for.
///
/// # Returns - (int32):
/// The handle of the found entity, or INVALID_EHANDLE_INDEX if none found.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn FindEntityByClassname(startFrom: i32, classname: &Str) -> i32 {
    unsafe { __s2sdk_FindEntityByClassname.expect("FindEntityByClassname function was not found")(startFrom, classname) }
}
pub type _FindEntityByClassname = unsafe extern "C" fn(i32, &Str) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_FindEntityByClassname: Option<_FindEntityByClassname> = None;

/// Creates an entity by string name but does not spawn it.
///
/// # Arguments
/// * `className` - (string): The class name of the entity to create.
///
/// # Returns - (int32):
/// The entity handle of the created entity, or INVALID_EHANDLE_INDEX if the entity could not be created.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn CreateEntityByName(className: &Str) -> i32 {
    unsafe { __s2sdk_CreateEntityByName.expect("CreateEntityByName function was not found")(className) }
}
pub type _CreateEntityByName = unsafe extern "C" fn(&Str) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_CreateEntityByName: Option<_CreateEntityByName> = None;

/// Spawns an entity into the game.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity to spawn.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn DispatchSpawn(entityHandle: i32) {
    unsafe { __s2sdk_DispatchSpawn.expect("DispatchSpawn function was not found")(entityHandle) }
}
pub type _DispatchSpawn = unsafe extern "C" fn(i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_DispatchSpawn: Option<_DispatchSpawn> = None;

/// Spawns an entity into the game with key-value properties.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity to spawn.
/// * `keys` - (string[]): A vector of keys representing the property names to set on the entity.
/// * `values` - (any[]): A vector of values corresponding to the keys, representing the property values to set on the entity.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn DispatchSpawn2(entityHandle: i32, keys: &Arr<Str>, values: &Arr<Var>) {
    unsafe { __s2sdk_DispatchSpawn2.expect("DispatchSpawn2 function was not found")(entityHandle, keys, values) }
}
pub type _DispatchSpawn2 = unsafe extern "C" fn(i32, &Arr<Str>, &Arr<Var>);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_DispatchSpawn2: Option<_DispatchSpawn2> = None;

/// Retrieves the movement type of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose movement type is to be retrieved.
///
/// # Returns - (int32):
/// The movement type of the entity, or 0 if the entity is invalid.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEntityMoveType(entityHandle: i32) -> MoveType {
    unsafe { __s2sdk_GetEntityMoveType.expect("GetEntityMoveType function was not found")(entityHandle) }
}
pub type _GetEntityMoveType = unsafe extern "C" fn(i32) -> MoveType;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEntityMoveType: Option<_GetEntityMoveType> = None;

/// Sets the movement type of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose movement type is to be set.
/// * `moveType` - (int32): The new movement type to set for the entity.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetEntityMoveType(entityHandle: i32, moveType: MoveType) {
    unsafe { __s2sdk_SetEntityMoveType.expect("SetEntityMoveType function was not found")(entityHandle, moveType) }
}
pub type _SetEntityMoveType = unsafe extern "C" fn(i32, MoveType);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetEntityMoveType: Option<_SetEntityMoveType> = None;

/// Sets the gravity scale of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose gravity scale is to be set.
/// * `gravity` - (float): The new gravity scale to set for the entity.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetEntityGravity(entityHandle: i32, gravity: f32) {
    unsafe { __s2sdk_SetEntityGravity.expect("SetEntityGravity function was not found")(entityHandle, gravity) }
}
pub type _SetEntityGravity = unsafe extern "C" fn(i32, f32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetEntityGravity: Option<_SetEntityGravity> = None;

/// Sets the owner of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose owner is to be set.
/// * `ownerHandle` - (int32): The handle of the new owner entity.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetEntityOwner(entityHandle: i32, ownerHandle: i32) {
    unsafe { __s2sdk_SetEntityOwner.expect("SetEntityOwner function was not found")(entityHandle, ownerHandle) }
}
pub type _SetEntityOwner = unsafe extern "C" fn(i32, i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetEntityOwner: Option<_SetEntityOwner> = None;

/// Retrieves the parent of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose parent is to be retrieved.
///
/// # Returns - (int32):
/// The handle of the parent entity, or INVALID_EHANDLE_INDEX if the entity is invalid.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEntityParent(entityHandle: i32) -> i32 {
    unsafe { __s2sdk_GetEntityParent.expect("GetEntityParent function was not found")(entityHandle) }
}
pub type _GetEntityParent = unsafe extern "C" fn(i32) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEntityParent: Option<_GetEntityParent> = None;

/// Sets the parent of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose parent is to be set.
/// * `parentHandle` - (int32): The handle of the new parent entity. (Can be invalid to clean parent)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetEntityParent(entityHandle: i32, parentHandle: i32) {
    unsafe { __s2sdk_SetEntityParent.expect("SetEntityParent function was not found")(entityHandle, parentHandle) }
}
pub type _SetEntityParent = unsafe extern "C" fn(i32, i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetEntityParent: Option<_SetEntityParent> = None;

/// Sets the parent of an entity to attachment by name.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose parent is to be set.
/// * `parentHandle` - (int32): The handle of the new parent entity.
/// * `attachmentName` - (string): The name of the entity's attachment.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetEntityParentAttachment(entityHandle: i32, parentHandle: i32, attachmentName: &Str) {
    unsafe {
        __s2sdk_SetEntityParentAttachment.expect("SetEntityParentAttachment function was not found")(entityHandle, parentHandle, attachmentName)
    }
}
pub type _SetEntityParentAttachment = unsafe extern "C" fn(i32, i32, &Str);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetEntityParentAttachment: Option<_SetEntityParentAttachment> = None;

/// Retrieves the absolute origin of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose absolute origin is to be retrieved.
///
/// # Returns - (vec3):
/// A vector where the absolute origin will be stored.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEntityAbsOrigin(entityHandle: i32) -> Vec3 {
    unsafe { __s2sdk_GetEntityAbsOrigin.expect("GetEntityAbsOrigin function was not found")(entityHandle) }
}
pub type _GetEntityAbsOrigin = unsafe extern "C" fn(i32) -> Vec3;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEntityAbsOrigin: Option<_GetEntityAbsOrigin> = None;

/// Sets the absolute origin of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose absolute origin is to be set.
/// * `origin` - (vec3): The new absolute origin to set for the entity.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetEntityAbsOrigin(entityHandle: i32, origin: &Vec3) {
    unsafe { __s2sdk_SetEntityAbsOrigin.expect("SetEntityAbsOrigin function was not found")(entityHandle, origin) }
}
pub type _SetEntityAbsOrigin = unsafe extern "C" fn(i32, &Vec3);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetEntityAbsOrigin: Option<_SetEntityAbsOrigin> = None;

/// Retrieves the local origin of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose local origin is to be retrieved.
///
/// # Returns - (vec3):
/// A vector where the local origin will be stored.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEntityLocalOrigin(entityHandle: i32) -> Vec3 {
    unsafe { __s2sdk_GetEntityLocalOrigin.expect("GetEntityLocalOrigin function was not found")(entityHandle) }
}
pub type _GetEntityLocalOrigin = unsafe extern "C" fn(i32) -> Vec3;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEntityLocalOrigin: Option<_GetEntityLocalOrigin> = None;

/// Sets the local origin of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose local origin is to be set.
/// * `origin` - (vec3): The new local origin to set for the entity.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetEntityLocalOrigin(entityHandle: i32, origin: &Vec3) {
    unsafe { __s2sdk_SetEntityLocalOrigin.expect("SetEntityLocalOrigin function was not found")(entityHandle, origin) }
}
pub type _SetEntityLocalOrigin = unsafe extern "C" fn(i32, &Vec3);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetEntityLocalOrigin: Option<_SetEntityLocalOrigin> = None;

/// Retrieves the absolute velocity of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose absolute velocity is to be retrieved.
///
/// # Returns - (vec3):
/// A vector where the absolute velocity will be stored.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEntityAbsVelocity(entityHandle: i32) -> Vec3 {
    unsafe { __s2sdk_GetEntityAbsVelocity.expect("GetEntityAbsVelocity function was not found")(entityHandle) }
}
pub type _GetEntityAbsVelocity = unsafe extern "C" fn(i32) -> Vec3;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEntityAbsVelocity: Option<_GetEntityAbsVelocity> = None;

/// Sets the absolute velocity of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose absolute velocity is to be set.
/// * `velocity` - (vec3): The new absolute velocity to set for the entity.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetEntityAbsVelocity(entityHandle: i32, velocity: &Vec3) {
    unsafe { __s2sdk_SetEntityAbsVelocity.expect("SetEntityAbsVelocity function was not found")(entityHandle, velocity) }
}
pub type _SetEntityAbsVelocity = unsafe extern "C" fn(i32, &Vec3);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetEntityAbsVelocity: Option<_SetEntityAbsVelocity> = None;

/// Sets the angular velocity of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose angular velocity is to be set.
/// * `velocity` - (vec3): The new angular velocity to set for the entity.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetEntityAngVelocity(entityHandle: i32, velocity: &Vec3) {
    unsafe { __s2sdk_SetEntityAngVelocity.expect("SetEntityAngVelocity function was not found")(entityHandle, velocity) }
}
pub type _SetEntityAngVelocity = unsafe extern "C" fn(i32, &Vec3);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetEntityAngVelocity: Option<_SetEntityAngVelocity> = None;

/// Sets the model name of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose model name is to be set.
/// * `model` - (string): The new model name to set for the entity.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetEntityModel(entityHandle: i32, model: &Str) {
    unsafe { __s2sdk_SetEntityModel.expect("SetEntityModel function was not found")(entityHandle, model) }
}
pub type _SetEntityModel = unsafe extern "C" fn(i32, &Str);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetEntityModel: Option<_SetEntityModel> = None;

/// Teleports an entity to a specified location and orientation.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity to teleport.
/// * `origin` - (vec3): A pointer to a Vector representing the new absolute position. Use nan vector to not set.
/// * `angles` - (vec3): A pointer to a QAngle representing the new orientation. Use nan vector to not set.
/// * `velocity` - (vec3): A pointer to a Vector representing the new velocity. Use nan vector to not set.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn TeleportEntity(entityHandle: i32, origin: &Vec3, angles: &Vec3, velocity: &Vec3) {
    unsafe { __s2sdk_TeleportEntity.expect("TeleportEntity function was not found")(entityHandle, origin, angles, velocity) }
}
pub type _TeleportEntity = unsafe extern "C" fn(i32, &Vec3, &Vec3, &Vec3);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_TeleportEntity: Option<_TeleportEntity> = None;

/// Invokes a named input method on a specified entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the target entity that will receive the input.
/// * `inputName` - (string): The name of the input action to invoke.
/// * `activatorHandle` - (int32): The handle of the entity that initiated the sequence of actions.
/// * `callerHandle` - (int32): The handle of the entity sending this event.
/// * `value` - (any): The value associated with the input action.
/// * `type_` - (int32): The type or classification of the value.
/// * `outputId` - (int32): An identifier for tracking the output of this operation.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn AcceptEntityInput(entityHandle: i32, inputName: &Str, activatorHandle: i32, callerHandle: i32, value: &Var, type_: FieldType, outputId: i32) {
    unsafe {
        __s2sdk_AcceptEntityInput.expect("AcceptEntityInput function was not found")(
            entityHandle,
            inputName,
            activatorHandle,
            callerHandle,
            value,
            type_,
            outputId,
        )
    }
}
pub type _AcceptEntityInput = unsafe extern "C" fn(i32, &Str, i32, i32, &Var, FieldType, i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_AcceptEntityInput: Option<_AcceptEntityInput> = None;

/// Retrieves the angular rotation of an entity.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity whose angular rotation is to be retrieved.
///
/// # Returns - (vec3):
/// A vector where the angular rotation will be stored.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEntityAngRotation(entityHandle: i32) -> Vec3 {
    unsafe { __s2sdk_GetEntityAngRotation.expect("GetEntityAngRotation function was not found")(entityHandle) }
}
pub type _GetEntityAngRotation = unsafe extern "C" fn(i32) -> Vec3;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEntityAngRotation: Option<_GetEntityAngRotation> = None;

/// Checks if an entity is a player pawn.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity.
///
/// # Returns - (bool):
/// True if the entity is a player pawn, false otherwise.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn IsEntityPlayerPawn(entityHandle: i32) -> bool {
    unsafe { __s2sdk_IsEntityPlayerPawn.expect("IsEntityPlayerPawn function was not found")(entityHandle) }
}
pub type _IsEntityPlayerPawn = unsafe extern "C" fn(i32) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_IsEntityPlayerPawn: Option<_IsEntityPlayerPawn> = None;

/// Checks if an entity is a player controller.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity.
///
/// # Returns - (bool):
/// True if the entity is a player controller, false otherwise.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn IsEntityPlayerController(entityHandle: i32) -> bool {
    unsafe { __s2sdk_IsEntityPlayerController.expect("IsEntityPlayerController function was not found")(entityHandle) }
}
pub type _IsEntityPlayerController = unsafe extern "C" fn(i32) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_IsEntityPlayerController: Option<_IsEntityPlayerController> = None;

/// Get vector to eye position - absolute coords.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity
///
/// # Returns - (vec3):
/// Eye position in absolute/world coordinates
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEntityEyePosition(entityHandle: i32) -> Vec3 {
    unsafe { __s2sdk_GetEntityEyePosition.expect("GetEntityEyePosition function was not found")(entityHandle) }
}
pub type _GetEntityEyePosition = unsafe extern "C" fn(i32) -> Vec3;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEntityEyePosition: Option<_GetEntityEyePosition> = None;

/// Get the qangles that this entity is looking at.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity
///
/// # Returns - (vec3):
/// Eye angles as a vector (pitch, yaw, roll)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEntityEyeAngles(entityHandle: i32) -> Vec3 {
    unsafe { __s2sdk_GetEntityEyeAngles.expect("GetEntityEyeAngles function was not found")(entityHandle) }
}
pub type _GetEntityEyeAngles = unsafe extern "C" fn(i32) -> Vec3;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEntityEyeAngles: Option<_GetEntityEyeAngles> = None;
