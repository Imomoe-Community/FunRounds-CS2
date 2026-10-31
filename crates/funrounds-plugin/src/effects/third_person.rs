// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    capability::{Probe, imports},
    fault::{Fault, Mechanism},
    funrounds_native as native,
    game::LivePlayer,
    s2sdk,
};
use funrounds_core::ThirdPersonSettings;
use plugify::{Str, Vec3};

const CAMERA: &str = "CCSCustomPlayerCamera";
const SERVICES: &str = "CPlayer_CameraServices";

pub(crate) fn probe() -> Probe {
    let mut probe = Probe::default();
    imports!(probe;
        __s2sdk_CreateEntityByName, __s2sdk_DispatchSpawn, __s2sdk_RemoveEntity,
        __s2sdk_EntHandleToEntPointer, __s2sdk_EntPointerToEntHandle, __s2sdk_IsValidEntHandle,
        __s2sdk_GetEntSchema2, __s2sdk_SetEntSchema2, __s2sdk_GetEntSchemaEnt2, __s2sdk_SetEntSchemaEnt2,
        __s2sdk_GetEntSchemaVector3D2, __s2sdk_SetEntSchemaVector3D2,
        __s2sdk_GetEntSchemaFloat2, __s2sdk_SetEntSchemaFloat2,
        __s2sdk_GetMaxClients, __s2sdk_IsClientInGame, __s2sdk_GetClientPawn,
    );
    for field in [
        "m_hPawn",
        "m_nCameraMode",
        "m_hFollowEntity",
        "m_bFollowEyes",
        "m_vecFollowOffset",
        "m_vecCameraOffset",
        "m_bClipCameraOffset",
        "m_flCameraOffsetReturnStrength",
    ] {
        probe.schema(CAMERA, field);
    }
    probe.schema("CBasePlayerPawn", "m_pCameraServices");
    probe.schema(SERVICES, "m_hViewEntity");
    probe.require(
        unsafe { native::__funrounds_native_RegisterThirdPersonEyeTarget }.is_some(),
        "native:RegisterThirdPersonEyeTarget",
    );
    probe.require(
        unsafe { native::__funrounds_native_UnregisterThirdPersonEyeTarget }.is_some(),
        "native:UnregisterThirdPersonEyeTarget",
    );
    probe.require(
        unsafe { native::__funrounds_native_ResetThirdPersonEyeTargets }.is_some(),
        "native:ResetThirdPersonEyeTargets",
    );
    probe.require(
        unsafe { native::__funrounds_native_HasThirdPersonEyeSupport }.is_some_and(|query| unsafe { query() }),
        "native:ThirdPersonEyeCameraBranch",
    );
    probe
}

pub(crate) fn discard_map_targets() -> Result<(), String> {
    if unsafe { native::__funrounds_native_ResetThirdPersonEyeTargets }.is_none_or(|reset| unsafe { reset() }) {
        Ok(())
    } else {
        Err("third-person old-map Hook detach unconfirmed".into())
    }
}

fn pointer(handle: i32) -> Option<usize> {
    if !s2sdk::IsValidEntHandle(handle) {
        return None;
    }
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    (pointer != 0 && s2sdk::EntPointerToEntHandle(pointer) == handle).then_some(pointer)
}

fn services(pawn: usize) -> Result<usize, String> {
    let service = s2sdk::GetEntSchema2(pawn, &Str::from_str("CBasePlayerPawn"), &Str::from_str("m_pCameraServices"), 0) as usize;
    if service == 0 {
        Err("third-person camera services unavailable".into())
    } else {
        Ok(service)
    }
}

fn view(pawn: usize) -> Result<i32, String> {
    Ok(s2sdk::GetEntSchemaEnt2(
        services(pawn)?,
        &Str::from_str(SERVICES),
        &Str::from_str("m_hViewEntity"),
        0,
    ))
}

fn set_view(pawn: usize, handle: i32) -> Result<(), String> {
    s2sdk::SetEntSchemaEnt2(
        services(pawn)?,
        &Str::from_str(SERVICES),
        &Str::from_str("m_hViewEntity"),
        handle,
        true,
        0,
    );
    if view(pawn)? == handle {
        Ok(())
    } else {
        Err("third-person view write readback failed".into())
    }
}

fn referenced(camera: i32) -> Result<bool, String> {
    for slot in 0..s2sdk::GetMaxClients() {
        if !s2sdk::IsClientInGame(slot) {
            continue;
        }
        let pawn = s2sdk::GetClientPawn(slot);
        if pawn != 0 && view(pawn)? == camera {
            return Ok(true);
        }
    }
    Ok(false)
}

#[derive(Debug)]
struct Camera {
    pawn: i32,
    handle: i32,
    previous_view: i32,
    registration_pending: bool,
}

#[derive(Default, Debug)]
pub(crate) struct ThirdPerson {
    camera: Option<Camera>,
}

impl ThirdPerson {
    pub(crate) fn needs_update(&self, player: LivePlayer) -> bool {
        let Some(pawn) = pointer(player.pawn_handle) else { return true };
        let Ok(current_view) = view(pawn) else { return true };
        match &self.camera {
            Some(camera) => current_view != camera.handle || pointer(camera.handle).is_none() || !camera.registration_pending,
            None => !s2sdk::IsValidEntHandle(current_view),
        }
    }

    pub(crate) fn update(&mut self, player: LivePlayer, settings: ThirdPersonSettings, current: &impl Fn() -> bool) -> Result<(), Fault> {
        let check = || {
            if current() && LivePlayer::resolve(player.slot) == Some(player) {
                Ok(())
            } else {
                Err(Fault::cancelled("third-person action identity changed"))
            }
        };
        check()?;
        let pawn = pointer(player.pawn_handle).ok_or_else(|| Fault::cancelled("third-person pawn expired"))?;
        let current_view = view(pawn).map_err(camera_fault)?;
        if let Some(camera) = &self.camera {
            if camera.pawn != player.pawn_handle {
                return Err(Fault::cancelled("third-person captured pawn changed"));
            }
            if current_view == camera.handle && pointer(camera.handle).is_some() {
                return Ok(());
            }
            // An external view takes priority. Restore never writes over it.
            if !self.release().map_err(camera_fault)? {
                return Ok(());
            }
            check()?;
        }
        if s2sdk::IsValidEntHandle(view(pawn).map_err(camera_fault)?) {
            return Ok(());
        }
        let previous_view = view(pawn).map_err(camera_fault)?;
        let handle = s2sdk::CreateEntityByName(&Str::from_str("custom_player_camera"));
        if !s2sdk::IsValidEntHandle(handle) {
            return Err(camera_fault("third-person camera creation failed"));
        }
        self.camera = Some(Camera {
            pawn: player.pawn_handle,
            handle,
            previous_view,
            registration_pending: false,
        });
        check()?;
        s2sdk::DispatchSpawn(handle);
        check()?;
        let camera = pointer(handle).ok_or_else(|| camera_fault("third-person camera disappeared during spawn"))?;
        // Spawn initializes m_nCameraMode back to its default; configure the live entity afterward.
        configure(camera, player.pawn_handle, settings)?;
        // Recheck external ownership after spawn before registering and assigning.
        if view(pawn).map_err(camera_fault)? != previous_view {
            return self.release().map(|_| ()).map_err(camera_fault);
        }
        self.camera.as_mut().expect("captured camera").registration_pending = true;
        let registered = unsafe { native::__funrounds_native_RegisterThirdPersonEyeTarget }
            .is_some_and(|register| unsafe { register(player.pawn_handle, handle) });
        if !registered {
            return Err(camera_fault("third-person eye Hook registration failed"));
        }
        check()?;
        set_view(pawn, handle).map_err(camera_fault)
    }

    pub(crate) fn restore(&mut self) -> Result<(), String> {
        self.release().map(|_| ())
    }

    pub(crate) fn is_clean(&self) -> bool {
        self.camera.is_none()
    }

    fn release(&mut self) -> Result<bool, String> {
        let Some(camera) = &mut self.camera else { return Ok(true) };
        if let Some(pawn) = pointer(camera.pawn)
            && view(pawn)? == camera.handle
        {
            // Keep the Hook target if the view write fails: correction is still needed.
            set_view(pawn, camera.previous_view)?;
        }
        if referenced(camera.handle)? {
            return Ok(false);
        }
        if camera.registration_pending {
            let removed = unsafe { native::__funrounds_native_UnregisterThirdPersonEyeTarget }
                .is_some_and(|unregister| unsafe { unregister(camera.pawn, camera.handle) });
            if !removed {
                return Err("third-person eye target/detour removal unconfirmed".into());
            }
            camera.registration_pending = false;
        }
        if pointer(camera.handle).is_some() {
            s2sdk::RemoveEntity(camera.handle);
            // Reuse the engine deletion acknowledgement used by bombs and night vision.
            // A queued, detached camera may retain its full handle through round_prestart.
            if !crate::game::engine_removal_acknowledged(camera.handle) {
                return Ok(false);
            }
        }
        self.camera = None;
        Ok(true)
    }
}

fn camera_fault(message: impl Into<String>) -> Fault {
    Fault::mechanism(Mechanism::ThirdPerson, message)
}

fn configure(camera: usize, pawn: i32, settings: ThirdPersonSettings) -> Result<(), Fault> {
    let class = Str::from_str(CAMERA);
    for (field, value) in [("m_hPawn", pawn), ("m_hFollowEntity", pawn)] {
        s2sdk::SetEntSchemaEnt2(camera, &class, &Str::from_str(field), value, true, 0);
    }
    for (field, value) in [
        ("m_nCameraMode", 3),
        ("m_bFollowEyes", i64::from(settings.follow_eyes)),
        ("m_bClipCameraOffset", i64::from(settings.clip_camera_offset)),
    ] {
        s2sdk::SetEntSchema2(camera, &class, &Str::from_str(field), value, true, 0);
    }
    s2sdk::SetEntSchemaVector3D2(camera, &class, &Str::from_str("m_vecFollowOffset"), &Vec3::default(), true, 0);
    s2sdk::SetEntSchemaVector3D2(
        camera,
        &class,
        &Str::from_str("m_vecCameraOffset"),
        &Vec3 {
            x: -settings.distance,
            y: 0.0,
            z: 0.0,
        },
        true,
        0,
    );
    s2sdk::SetEntSchemaFloat2(
        camera,
        &class,
        &Str::from_str("m_flCameraOffsetReturnStrength"),
        f64::from(settings.return_strength),
        true,
        0,
    );
    verify(camera, pawn, settings)
}

fn verify(camera: usize, pawn: i32, settings: ThirdPersonSettings) -> Result<(), Fault> {
    let class = Str::from_str(CAMERA);
    let follow = s2sdk::GetEntSchemaVector3D2(camera, &class, &Str::from_str("m_vecFollowOffset"), 0);
    let offset = s2sdk::GetEntSchemaVector3D2(camera, &class, &Str::from_str("m_vecCameraOffset"), 0);
    let valid = ["m_hPawn", "m_hFollowEntity"]
        .iter()
        .all(|field| s2sdk::GetEntSchemaEnt2(camera, &class, &Str::from_str(field), 0) == pawn)
        && s2sdk::GetEntSchema2(camera, &class, &Str::from_str("m_nCameraMode"), 0) == 3
        && (s2sdk::GetEntSchema2(camera, &class, &Str::from_str("m_bFollowEyes"), 0) != 0) == settings.follow_eyes
        && (s2sdk::GetEntSchema2(camera, &class, &Str::from_str("m_bClipCameraOffset"), 0) != 0) == settings.clip_camera_offset
        && follow.x == 0.0
        && follow.y == 0.0
        && follow.z == 0.0
        && offset.x == -settings.distance
        && offset.y == 0.0
        && offset.z == 0.0
        && (s2sdk::GetEntSchemaFloat2(camera, &class, &Str::from_str("m_flCameraOffsetReturnStrength"), 0) - f64::from(settings.return_strength))
            .abs()
            < 0.0001;
    if valid {
        Ok(())
    } else {
        Err(camera_fault("third-person camera schema readback failed"))
    }
}
