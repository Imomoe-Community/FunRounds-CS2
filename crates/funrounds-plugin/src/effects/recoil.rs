// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    fault::{Fault, Mechanism},
    game::LivePlayer,
    s2sdk,
};
use funrounds_core::Mode;
use plugify::{Str, Vec3};

const AIM_PUNCH_COMPONENT: &str = "CCSPlayer_AimPunchServices";
const AIM_PUNCH_FIELDS: &[&str] = &[
    "m_predictableBaseTick",
    "m_predictableBaseAngle",
    "m_predictableBaseAngleVel",
    "m_unpredictableBaseTick",
    "m_unpredictableBaseAngle",
];
const WEAPON_RECOIL_FIELDS: &[&str] = &["m_flRecoilIndex", "m_fAccuracyPenalty"];
const SHAKE_MESSAGE: &str = "CUserMessageShake";
const SHAKE_START_COMMAND: u32 = 0;
const SHAKE_FREQUENCY: f32 = 12.0;
const SHAKE_DURATION: f32 = 0.2;
const NON_FIREARM_WEAPONS: &[&str] = &[
    "weapon_c4",
    "weapon_knife",
    "weapon_knife_t",
    "weapon_decoy",
    "weapon_flashbang",
    "weapon_hegrenade",
    "weapon_incgrenade",
    "weapon_molotov",
    "weapon_smokegrenade",
];

#[derive(Clone, Copy)]
pub(super) struct AimPunchState {
    predictable_tick: i64,
    predictable_angle: Vec3,
    predictable_angle_velocity: Vec3,
    unpredictable_tick: i64,
    unpredictable_angle: Vec3,
}

#[derive(Clone)]
pub(super) struct WeaponRecoilState {
    handle: i32,
    classname: String,
    recoil_index: f64,
    accuracy_penalty: f64,
}

impl super::Effect {
    pub fn restore_weapon_recoil(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        for weapon in &self.weapon_recoil {
            if let Err(error) = weapon.restore() {
                errors.push(error);
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
    }

    pub fn settle_recoil(&self) -> Result<Option<String>, Fault> {
        // weapon_fire is dispatched before the engine finishes adding the shot's view-punch velocity.
        let before = AimPunchState::read(self.player)?;
        before.without_angles().write(self.player)?;
        Ok(self.diagnostics.then(|| {
            format!(
                "post-shot cleared slot={} previous_aim_vel={} ticks={}/{}",
                self.player.slot,
                format_vec3(before.predictable_angle_velocity),
                before.predictable_tick,
                before.unpredictable_tick,
            )
        }))
    }

    pub fn observe_recoil(&self) -> Result<String, String> {
        let state = AimPunchState::read(self.player).map_err(|error| error.to_string())?;
        Ok(format!(
            "post-shot slot={} aim_tick={}/{} aim={}/{} aim_vel={}",
            self.player.slot,
            state.predictable_tick,
            state.unpredictable_tick,
            format_vec3(state.predictable_angle),
            format_vec3(state.unpredictable_angle),
            format_vec3(state.predictable_angle_velocity),
        ))
    }

    pub fn observe_ammo(&self, handle: i32) -> Result<String, String> {
        if LivePlayer::resolve(self.player.slot) != Some(self.player)
            || !s2sdk::GetClientWeapons(self.player.slot).iter().any(|weapon| *weapon == handle)
        {
            return Err("shot weapon no longer belongs to the original player".into());
        }
        let classname = s2sdk::GetEntityClassname(handle).to_string();
        let pointer = weapon_pointer(handle, &classname)?;
        let (clip, reserve) = ammo_counts(pointer);
        Ok(format!(
            "post-shot ammo slot={} weapon={classname} handle={handle} clip={} reserve_primary={} tick={} time={}",
            self.player.slot,
            display_count(clip),
            display_count(reserve),
            s2sdk::GetGameTickCount(),
            s2sdk::GetGameTime()
        ))
    }

    pub fn weapon_fire(&mut self, player: LivePlayer, mode: &Mode) -> Result<Option<String>, Fault> {
        if player != self.player || LivePlayer::resolve(player.slot) != Some(player) {
            return Err(Fault::cancelled("player identity changed before shooting effects"));
        }
        let handle = s2sdk::GetClientActiveWeapon(player.slot);
        if !s2sdk::IsValidEntHandle(handle) || !s2sdk::GetClientWeapons(player.slot).iter().any(|weapon| *weapon == handle) {
            return Err(Fault::cancelled("weapon_fire had no validated active weapon"));
        }
        let classname = s2sdk::GetEntityClassname(handle).to_string();
        let weapon = weapon_pointer(handle, &classname).map_err(Fault::cancelled)?;
        let shooting_state = if mode.no_spread_recoil && is_firearm(&classname) {
            let cleared_aim_punch = AimPunchState::read(player)?.without_angles();
            cleared_aim_punch.write(player)?;
            self.capture_weapon_recoil(handle)?;
            let weapon_recoil = self.clear_weapon_recoil(handle, &classname)?;
            if self.diagnostics {
                let now = AimPunchState::read(player)?;
                format!(
                    " aim_tick={}/{} aim={}/{} aim_vel={} recoil={:.3}/{:.3}",
                    now.predictable_tick,
                    now.unpredictable_tick,
                    format_vec3(now.predictable_angle),
                    format_vec3(now.unpredictable_angle),
                    format_vec3(now.predictable_angle_velocity),
                    weapon_recoil.0,
                    weapon_recoil.1,
                )
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        let shake = if let Some(view) = mode.recoil_view.filter(|_| is_firearm(&classname)) {
            let amplitude = shake_amplitude(view);
            send_recoil_shake(player.slot, amplitude).map_err(|error| Fault::mechanism(Mechanism::RecoilView, error))?;
            Some(amplitude)
        } else {
            None
        };
        Ok(self.diagnostics.then(|| {
            let (clip, reserve) = ammo_counts(weapon);
            let shake = shake.map_or_else(|| "none".to_owned(), |amplitude| format!("requested:{amplitude:.3}"));
            format!(
                "weapon_fire mode={} slot={} weapon={} handle={} clip={} reserve_primary={} tick={} time={} view_shake={shake}{shooting_state}",
                mode.id,
                player.slot,
                classname,
                handle,
                display_count(clip),
                display_count(reserve),
                s2sdk::GetGameTickCount(),
                s2sdk::GetGameTime(),
            )
        }))
    }

    pub(super) fn capture_weapon_recoil(&mut self, handle: i32) -> Result<(), Fault> {
        if self.weapon_recoil.iter().any(|weapon| weapon.handle == handle) {
            return Ok(());
        }
        let classname = s2sdk::GetEntityClassname(handle).to_string();
        let pointer = weapon_pointer(handle, &classname).map_err(Fault::cancelled)?;
        ensure_weapon_recoil_fields().map_err(|error| Fault::mechanism(Mechanism::Recoil, error))?;
        let recoil_index = s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str("CCSWeaponBase"), &Str::from_str("m_flRecoilIndex"), 0);
        let accuracy_penalty = s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str("CCSWeaponBase"), &Str::from_str("m_fAccuracyPenalty"), 0);
        if !recoil_index.is_finite() || !accuracy_penalty.is_finite() {
            return Err(Fault::mechanism(
                Mechanism::Recoil,
                format!("invalid recoil fields on {classname} handle={handle}"),
            ));
        }
        self.weapon_recoil.push(WeaponRecoilState {
            handle,
            classname,
            recoil_index,
            accuracy_penalty,
        });
        Ok(())
    }

    fn clear_weapon_recoil(&self, handle: i32, classname: &str) -> Result<(f64, f64), Fault> {
        let pointer = weapon_pointer(handle, classname).map_err(Fault::cancelled)?;
        ensure_weapon_recoil_fields().map_err(|error| Fault::mechanism(Mechanism::Recoil, error))?;
        for field in WEAPON_RECOIL_FIELDS {
            s2sdk::SetEntSchemaFloat2(pointer, &Str::from_str("CCSWeaponBase"), &Str::from_str(field), 0.0, true, 0);
        }
        let recoil_index = s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str("CCSWeaponBase"), &Str::from_str("m_flRecoilIndex"), 0);
        let accuracy_penalty = s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str("CCSWeaponBase"), &Str::from_str("m_fAccuracyPenalty"), 0);
        if !recoil_index.is_finite() || !accuracy_penalty.is_finite() || recoil_index.abs() > 0.0001 || accuracy_penalty.abs() > 0.0001 {
            return Err(Fault::mechanism(
                Mechanism::Recoil,
                format!("recoil write verification failed for {classname} handle={handle}"),
            ));
        }
        Ok((recoil_index, accuracy_penalty))
    }
}

impl AimPunchState {
    fn without_angles(self) -> Self {
        Self {
            predictable_angle: Vec3 { x: 0.0, y: 0.0, z: 0.0 },
            predictable_angle_velocity: Vec3 { x: 0.0, y: 0.0, z: 0.0 },
            unpredictable_angle: Vec3 { x: 0.0, y: 0.0, z: 0.0 },
            ..self
        }
    }

    pub(super) fn capture(player: LivePlayer) -> Result<Self, Fault> {
        Self::read(player)
    }

    fn read(player: LivePlayer) -> Result<Self, Fault> {
        let services = aim_punch_services(player)?;
        ensure_aim_punch_fields().map_err(|error| Fault::mechanism(Mechanism::Recoil, error))?;
        let state = Self {
            predictable_tick: s2sdk::GetEntSchema2(services, &Str::from_str(AIM_PUNCH_COMPONENT), &Str::from_str("m_predictableBaseTick"), 0),
            predictable_angle: s2sdk::GetEntSchemaVector3D2(
                services,
                &Str::from_str(AIM_PUNCH_COMPONENT),
                &Str::from_str("m_predictableBaseAngle"),
                0,
            ),
            predictable_angle_velocity: s2sdk::GetEntSchemaVector3D2(
                services,
                &Str::from_str(AIM_PUNCH_COMPONENT),
                &Str::from_str("m_predictableBaseAngleVel"),
                0,
            ),
            unpredictable_tick: s2sdk::GetEntSchema2(
                services,
                &Str::from_str(AIM_PUNCH_COMPONENT),
                &Str::from_str("m_unpredictableBaseTick"),
                0,
            ),
            unpredictable_angle: s2sdk::GetEntSchemaVector3D2(
                services,
                &Str::from_str(AIM_PUNCH_COMPONENT),
                &Str::from_str("m_unpredictableBaseAngle"),
                0,
            ),
        };
        if [state.predictable_angle, state.predictable_angle_velocity, state.unpredictable_angle]
            .iter()
            .any(|value| !vec3_is_finite(*value))
        {
            return Err(Fault::mechanism(Mechanism::Recoil, "invalid aim punch schema values"));
        }
        Ok(state)
    }

    pub(super) fn write(self, player: LivePlayer) -> Result<(), Fault> {
        let services = aim_punch_services(player)?;
        ensure_aim_punch_fields().map_err(|error| Fault::mechanism(Mechanism::Recoil, error))?;
        s2sdk::SetEntSchema2(
            services,
            &Str::from_str(AIM_PUNCH_COMPONENT),
            &Str::from_str("m_predictableBaseTick"),
            self.predictable_tick,
            true,
            0,
        );
        s2sdk::SetEntSchemaVector3D2(
            services,
            &Str::from_str(AIM_PUNCH_COMPONENT),
            &Str::from_str("m_predictableBaseAngle"),
            &self.predictable_angle,
            true,
            0,
        );
        s2sdk::SetEntSchemaVector3D2(
            services,
            &Str::from_str(AIM_PUNCH_COMPONENT),
            &Str::from_str("m_predictableBaseAngleVel"),
            &self.predictable_angle_velocity,
            true,
            0,
        );
        s2sdk::SetEntSchema2(
            services,
            &Str::from_str(AIM_PUNCH_COMPONENT),
            &Str::from_str("m_unpredictableBaseTick"),
            self.unpredictable_tick,
            true,
            0,
        );
        s2sdk::SetEntSchemaVector3D2(
            services,
            &Str::from_str(AIM_PUNCH_COMPONENT),
            &Str::from_str("m_unpredictableBaseAngle"),
            &self.unpredictable_angle,
            true,
            0,
        );
        let actual = Self::read(player)?;
        if actual.predictable_tick != self.predictable_tick
            || !equal_vec3(actual.predictable_angle, self.predictable_angle)
            || !equal_vec3(actual.predictable_angle_velocity, self.predictable_angle_velocity)
            || actual.unpredictable_tick != self.unpredictable_tick
            || !equal_vec3(actual.unpredictable_angle, self.unpredictable_angle)
        {
            return Err(Fault::mechanism(Mechanism::Recoil, "aim punch write verification failed"));
        }
        Ok(())
    }
}

impl WeaponRecoilState {
    fn restore(&self) -> Result<(), String> {
        if !s2sdk::IsValidEntHandle(self.handle) {
            return Ok(());
        }
        let classname = s2sdk::GetEntityClassname(self.handle).to_string();
        if classname != self.classname {
            return Ok(());
        }
        let pointer = s2sdk::EntHandleToEntPointer(self.handle);
        if pointer == 0 || s2sdk::EntPointerToEntHandle(pointer) != self.handle {
            return Ok(());
        }
        ensure_weapon_recoil_fields()?;
        s2sdk::SetEntSchemaFloat2(
            pointer,
            &Str::from_str("CCSWeaponBase"),
            &Str::from_str("m_flRecoilIndex"),
            self.recoil_index,
            true,
            0,
        );
        s2sdk::SetEntSchemaFloat2(
            pointer,
            &Str::from_str("CCSWeaponBase"),
            &Str::from_str("m_fAccuracyPenalty"),
            self.accuracy_penalty,
            true,
            0,
        );
        let recoil_index = s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str("CCSWeaponBase"), &Str::from_str("m_flRecoilIndex"), 0);
        let accuracy_penalty = s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str("CCSWeaponBase"), &Str::from_str("m_fAccuracyPenalty"), 0);
        if !recoil_index.is_finite()
            || !accuracy_penalty.is_finite()
            || (recoil_index - self.recoil_index).abs() > 0.0001
            || (accuracy_penalty - self.accuracy_penalty).abs() > 0.0001
        {
            return Err(format!("recoil restoration failed for {} handle={}", self.classname, self.handle));
        }
        Ok(())
    }
}

fn aim_punch_services(player: LivePlayer) -> Result<usize, Fault> {
    if LivePlayer::resolve(player.slot) != Some(player) {
        return Err(Fault::cancelled("player identity changed before aim punch access"));
    }
    let pawn = s2sdk::EntHandleToEntPointer(player.pawn_handle);
    if pawn == 0 || s2sdk::EntPointerToEntHandle(pawn) != player.pawn_handle {
        return Err(Fault::cancelled("player pawn handle changed before aim punch access"));
    }
    if s2sdk::GetSchemaOffset(&Str::from_str("CCSPlayerPawn"), &Str::from_str("m_pAimPunchServices")) < 0 {
        return Err(Fault::mechanism(
            Mechanism::Recoil,
            "CCSPlayerPawn.m_pAimPunchServices schema unavailable",
        ));
    }
    let services = s2sdk::GetEntSchema2(pawn, &Str::from_str("CCSPlayerPawn"), &Str::from_str("m_pAimPunchServices"), 0) as usize;
    if services == 0 {
        Err(Fault::mechanism(Mechanism::Recoil, "player aim punch services unavailable"))
    } else {
        Ok(services)
    }
}

fn ensure_aim_punch_fields() -> Result<(), String> {
    for field in AIM_PUNCH_FIELDS {
        if s2sdk::GetSchemaOffset(&Str::from_str(AIM_PUNCH_COMPONENT), &Str::from_str(field)) < 0 {
            return Err(format!("{AIM_PUNCH_COMPONENT}.{field} schema unavailable"));
        }
    }
    Ok(())
}

fn ensure_weapon_recoil_fields() -> Result<(), String> {
    for field in WEAPON_RECOIL_FIELDS {
        if s2sdk::GetSchemaOffset(&Str::from_str("CCSWeaponBase"), &Str::from_str(field)) < 0 {
            return Err(format!("CCSWeaponBase.{field} schema unavailable"));
        }
    }
    Ok(())
}

fn weapon_pointer(handle: i32, classname: &str) -> Result<usize, String> {
    if !s2sdk::IsValidEntHandle(handle) {
        return Err(format!("invalid weapon handle={handle}"));
    }
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    if pointer == 0 || s2sdk::EntPointerToEntHandle(pointer) != handle || s2sdk::GetEntityClassname(handle).to_string() != classname {
        return Err(format!("weapon handle changed before schema access: {classname} handle={handle}"));
    }
    Ok(pointer)
}

pub(super) fn is_firearm(classname: &str) -> bool {
    classname.starts_with("weapon_") && !NON_FIREARM_WEAPONS.contains(&classname)
}

pub(crate) fn firearm_event(weapon: &str) -> bool {
    if weapon.is_empty() {
        return false;
    }
    let classname = if weapon.starts_with("weapon_") {
        weapon.to_owned()
    } else {
        format!("weapon_{weapon}")
    };
    is_firearm(&classname)
}

fn ammo_counts(weapon: usize) -> (Option<i64>, Option<i64>) {
    let class = Str::from_str("CBasePlayerWeapon");
    if s2sdk::GetSchemaOffset(&class, &Str::from_str("m_iClip1")) < 0 || s2sdk::GetSchemaOffset(&class, &Str::from_str("m_pReserveAmmo")) < 0 {
        return (None, None);
    }
    (
        Some(s2sdk::GetEntSchema2(weapon, &class, &Str::from_str("m_iClip1"), 0)),
        Some(s2sdk::GetEntSchema2(weapon, &class, &Str::from_str("m_pReserveAmmo"), 0)),
    )
}

fn display_count(value: Option<i64>) -> String {
    value.map_or_else(|| "unavailable".into(), |count| count.to_string())
}

fn equal_vec3(left: Vec3, right: Vec3) -> bool {
    (left.x - right.x).abs() <= 0.0001 && (left.y - right.y).abs() <= 0.0001 && (left.z - right.z).abs() <= 0.0001
}

fn vec3_is_finite(value: Vec3) -> bool {
    value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
}

fn format_vec3(value: Vec3) -> String {
    format!("{:.3},{:.3},{:.3}", value.x, value.y, value.z)
}

fn shake_amplitude(recoil_view: f32) -> f32 {
    recoil_view / 10.0
}

fn send_recoil_shake(slot: i32, amplitude: f32) -> Result<(), String> {
    let message_id = s2sdk::UserMessageFindMessageIdByName(&Str::from_str(SHAKE_MESSAGE));
    if message_id <= 0 {
        return Err(format!("{SHAKE_MESSAGE} user message unavailable"));
    }
    let message = s2sdk::UserMessageCreateFromId(message_id);
    if message == 0 {
        return Err(format!("could not create {SHAKE_MESSAGE} user message"));
    }
    let result = (|| {
        if s2sdk::UserMessageGetMessageID(message) != message_id {
            return Err(format!("created user message ID changed for {SHAKE_MESSAGE}"));
        }
        if !s2sdk::PbSetUInt32(message, &Str::from_str("command"), SHAKE_START_COMMAND)
            || !s2sdk::PbSetFloat(message, &Str::from_str("amplitude"), amplitude)
            || !s2sdk::PbSetFloat(message, &Str::from_str("frequency"), SHAKE_FREQUENCY)
            || !s2sdk::PbSetFloat(message, &Str::from_str("duration"), SHAKE_DURATION)
        {
            return Err(format!("could not populate {SHAKE_MESSAGE} payload"));
        }
        s2sdk::UserMessageAddRecipient(message, slot);
        s2sdk::UserMessageSend(message);
        Ok(())
    })();
    s2sdk::UserMessageDestroy(message);
    result
}

pub(crate) fn recoil_schema_probe() -> crate::capability::Probe {
    let mut probe = crate::capability::Probe::default();
    probe.schema("CCSPlayerPawn", "m_pAimPunchServices");
    for field in AIM_PUNCH_FIELDS {
        probe.schema(AIM_PUNCH_COMPONENT, field);
    }
    for field in WEAPON_RECOIL_FIELDS {
        probe.schema("CCSWeaponBase", field);
    }
    probe
}

pub(crate) fn recoil_shake_probe() -> crate::capability::Probe {
    let mut probe = crate::capability::Probe::default();
    probe.require(
        s2sdk::UserMessageFindMessageIdByName(&Str::from_str(SHAKE_MESSAGE)) > 0,
        format!("usermessage:{SHAKE_MESSAGE}"),
    );
    probe
}

#[cfg(test)]
mod tests {
    #[test]
    fn shooting_events_accept_both_gun_names_but_not_throwing_or_objectives() {
        for name in ["ak47", "weapon_ssg08", "mag7", "weapon_taser"] {
            assert!(super::firearm_event(name));
        }
        for name in ["hegrenade", "weapon_smokegrenade", "knife", "knife_t", "c4", ""] {
            assert!(!super::firearm_event(name));
        }
    }
    #[test]
    fn no_recoil_includes_shotguns_and_excludes_non_firearms() {
        for name in ["weapon_mag7", "weapon_nova", "weapon_sawedoff", "weapon_xm1014"] {
            assert!(super::is_firearm(name), "{name} must receive recoil suppression");
        }
        for name in ["weapon_c4", "weapon_knife", "weapon_hegrenade", "weapon_smokegrenade"] {
            assert!(!super::is_firearm(name), "{name} must not receive recoil suppression");
        }
    }

    #[test]
    fn clearing_aim_punch_preserves_engine_ticks_and_clears_vectors() {
        let angle = super::Vec3 { x: 1.0, y: 2.0, z: 3.0 };
        let aim = super::AimPunchState {
            predictable_tick: 100,
            unpredictable_tick: 101,
            predictable_angle: angle,
            predictable_angle_velocity: angle,
            unpredictable_angle: angle,
        }
        .without_angles();
        assert_eq!(aim.predictable_tick, 100);
        assert_eq!(aim.unpredictable_tick, 101);
        for angle in [aim.predictable_angle, aim.predictable_angle_velocity, aim.unpredictable_angle] {
            assert_eq!((angle.x, angle.y, angle.z), (0.0, 0.0, 0.0));
        }
    }

    #[test]
    fn legacy_recoil_view_one_hundred_maps_to_a_strong_ten_unit_shake() {
        assert_eq!(super::shake_amplitude(100.0), 10.0);
        assert_eq!(super::SHAKE_START_COMMAND, 0);
    }
}
