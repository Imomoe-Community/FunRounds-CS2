// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    fault::{Fault, Mechanism},
    game::LivePlayer,
    s2sdk,
};
use funrounds_core::{Mode, PlayerIdentity};
use plugify::{Str, Vec3, Vec4};

pub(crate) mod equipment;
mod recoil;
mod render;
mod view;
mod weapon_rules;
use self::recoil::{AimPunchState, WeaponRecoilState};
pub(crate) use recoil::{firearm_event, recoil_schema_probe, recoil_shake_probe};
pub(crate) use render::probe as translucent_probe;
pub(crate) use view::{crosshair_probe, fov_probe, no_scope_probe};
pub(crate) use weapon_rules::{fast_switch_probe, single_shot_probe};

pub(crate) fn c4_removal_probe() -> crate::capability::Probe {
    let mut probe = crate::capability::Probe::default();
    crate::capability::imports!(probe;
        __s2sdk_GetClientWeapons, __s2sdk_RemoveWeapon, __s2sdk_IsValidEntHandle, __s2sdk_GetEntityClassname,
    );
    probe
}

#[unsafe(no_mangle)]
pub(crate) static mut __s2sdk_GetWeaponItemDefinition: Option<unsafe extern "C" fn(i32) -> u16> = None;

fn self_push_suppressed(buttons: u64, flags: i32) -> bool {
    buttons & (4 | 0x10000) != 0 || flags & 2 != 0
}

fn self_push_velocity(before: Vec3, angles: Vec3, force: f32) -> Vec3 {
    let pitch = angles.x.to_radians();
    let yaw = angles.y.to_radians();
    Vec3 {
        x: before.x * 0.3 - pitch.cos() * yaw.cos() * force,
        y: before.y * 0.3 - pitch.cos() * yaw.sin() * force,
        z: before.z * 0.3 + pitch.sin() * force,
    }
}

const MAX_HEALTH_CLASS: &str = "CBaseEntity";
const MAX_HEALTH_FIELD: &str = "m_iMaxHealth";

pub struct Effect {
    pub identity: PlayerIdentity,
    pub round: funrounds_core::RoundGeneration,
    player: LivePlayer,
    diagnostics: bool,
    health: i32,
    armor: i32,
    max_health: Option<i32>,
    helmet: bool,
    gravity: f32,
    speed: f32,
    base_speed: f32,
    speed_written: Option<f32>,
    movement_scale: Option<funrounds_core::MovementScale>,
    next_decay: f32,
    color: Option<Vec4>,
    aim_punch: Option<AimPunchState>,
    weapon_recoil: Vec<WeaponRecoilState>,
    replacement: Option<equipment::EquipmentReplacement>,
    upgraded_weapon: Option<(i32, String)>,
    translucent: Option<render::Translucent>,
    view: view::ViewEffects,
    weapon_rules: weapon_rules::WeaponRules,
}

impl Effect {
    pub fn capture(
        player: LivePlayer,
        identity: PlayerIdentity,
        round: funrounds_core::RoundGeneration,
        mode: &Mode,
        diagnostics: bool,
    ) -> Result<Self, Fault> {
        Ok(Self {
            identity,
            round,
            player,
            diagnostics,
            health: s2sdk::GetClientHealth(player.slot),
            armor: s2sdk::GetClientArmor(player.slot),
            max_health: mode_max_health(mode).map(|_| read_max_health(player)).transpose()?,
            helmet: player.helmet()?,
            gravity: s2sdk::GetClientGravity(player.slot),
            speed: s2sdk::GetClientSpeed(player.slot),
            base_speed: mode.speed,
            speed_written: None,
            movement_scale: None,
            next_decay: s2sdk::GetGameTime() + 2.0,
            color: mode.color.map(|_| s2sdk::GetClientRenderColor(player.slot)),
            aim_punch: mode.no_recoil.then(|| AimPunchState::capture(player)).transpose()?,
            weapon_recoil: Vec::new(),
            replacement: None,
            upgraded_weapon: None,
            translucent: mode.translucent.then(|| render::Translucent::new(player, identity, round)),
            view: view::ViewEffects::capture(player, identity, round, mode)?,
            weapon_rules: weapon_rules::WeaponRules::default(),
        })
    }

    pub fn update_view(&mut self, mode: &Mode, now: f32, round: funrounds_core::RoundGeneration) -> Result<(), Fault> {
        self.view.update(self.player, self.identity, round, mode, now)?;
        self.weapon_rules.update(self.player, mode)
    }

    pub fn release_inactive_scoped_weapon(&mut self, round: funrounds_core::RoundGeneration) -> Result<(), Fault> {
        self.view.release_inactive_secondary(self.player, self.identity, round)?;
        self.weapon_rules.release_unowned(self.player)
    }

    pub fn release_unowned_translucent_weapons(&mut self, round: funrounds_core::RoundGeneration) -> Result<(), Fault> {
        if let Some(translucent) = &mut self.translucent {
            translucent.release_unowned_weapons(round)?;
        }
        Ok(())
    }

    pub fn update_translucent(&mut self, mode: &Mode, round: funrounds_core::RoundGeneration) -> Result<(), Fault> {
        if !mode.translucent {
            if let Some(translucent) = &mut self.translucent {
                translucent.restore().map_err(|error| Fault::mechanism(Mechanism::Translucent, error))?;
            }
            self.translucent = None;
            return Ok(());
        }
        if let Some(translucent) = &mut self.translucent {
            translucent.update(self.identity, round)?;
        }
        Ok(())
    }

    pub fn update_classic(&mut self, mode: &Mode, now: f32, speed_owned: bool) -> Result<(), Fault> {
        if LivePlayer::resolve(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled("player identity changed before classic update"));
        }
        let slot = self.player.slot;
        let mut health = s2sdk::GetClientHealth(slot);
        if mode.health_decay && now >= self.next_decay {
            self.next_decay = now + 2.0;
            let decayed = funrounds_core::decay_health(health, mode.health);
            if decayed != health {
                s2sdk::SetClientHealth(slot, decayed);
                if s2sdk::GetClientHealth(slot) != decayed {
                    return Err(Fault::mechanism(Mechanism::HealthDecay, "classic health decay write failed"));
                }
                if self.diagnostics {
                    s2sdk::PrintToServer(&Str::from_str(&format!(
                        "[FunRounds] classic decay slot={slot} before={health} after={decayed}"
                    )));
                }
                health = decayed;
            }
        }
        if (mode.health_speed || mode.periodic_speed) && !speed_owned {
            let health_scale = if mode.health_speed {
                funrounds_core::health_scaling(health, mode.health).1
            } else {
                1.0
            };
            let periodic_scale = if mode.periodic_speed {
                self.movement_scale.map_or(1.0, |movement| movement.speed)
            } else {
                1.0
            };
            let speed = composed_speed(self.base_speed, health_scale, periodic_scale);
            if !speed_matches(s2sdk::GetClientSpeed(slot), speed) {
                self.speed_written = Some(speed);
                s2sdk::SetClientSpeed(slot, speed);
                if !speed_matches(s2sdk::GetClientSpeed(slot), speed) {
                    let mechanism = if mode.health_speed {
                        Mechanism::HealthSpeed
                    } else {
                        Mechanism::PeriodicSpeed
                    };
                    return Err(Fault::mechanism(mechanism, "classic speed write failed"));
                }
            }
        }
        Ok(())
    }

    pub fn update_periodic_movement(&mut self, movement: funrounds_core::MovementScale, mode: &Mode, speed_owned: bool) -> Result<(), Fault> {
        if LivePlayer::resolve(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled("player identity changed before periodic movement update"));
        }
        let periodic_speed = mode.periodic_speed.then(|| {
            let health_scale = if mode.health_speed {
                funrounds_core::health_scaling(s2sdk::GetClientHealth(self.player.slot), mode.health).1
            } else {
                1.0
            };
            composed_speed(self.base_speed, health_scale, movement.speed)
        });
        let periodic_gravity = mode.periodic_gravity.then_some(mode.gravity * movement.gravity);
        if self.movement_scale == Some(movement)
            && periodic_speed.is_none_or(|speed| speed_owned || speed_matches(s2sdk::GetClientSpeed(self.player.slot), speed))
            && periodic_gravity.is_none_or(|gravity| speed_matches(s2sdk::GetClientGravity(self.player.slot), gravity))
        {
            return Ok(());
        }
        if let Some(speed) = periodic_speed.filter(|_| !speed_owned)
            && !speed_matches(s2sdk::GetClientSpeed(self.player.slot), speed)
        {
            self.speed_written = Some(speed);
            s2sdk::SetClientSpeed(self.player.slot, speed);
        }
        if let Some(gravity) = periodic_gravity {
            s2sdk::SetClientGravity(self.player.slot, gravity);
        }
        if periodic_speed.is_some_and(|speed| !speed_owned && !speed_matches(s2sdk::GetClientSpeed(self.player.slot), speed)) {
            return Err(Fault::mechanism(Mechanism::PeriodicSpeed, "periodic speed write failed"));
        }
        if periodic_gravity.is_some_and(|gravity| !speed_matches(s2sdk::GetClientGravity(self.player.slot), gravity)) {
            return Err(Fault::mechanism(Mechanism::PeriodicGravity, "periodic gravity write failed"));
        }
        self.movement_scale = Some(movement);
        Ok(())
    }

    pub fn self_push(&self, player: LivePlayer, weapon: &str) -> Result<(), Fault> {
        if player != self.player || LivePlayer::resolve(player.slot) != Some(player) {
            return Err(Fault::cancelled("player identity changed before self-push"));
        }
        let name = if weapon.starts_with("weapon_") {
            weapon.to_owned()
        } else {
            format!("weapon_{weapon}")
        };
        let Some(force) = funrounds_core::self_push_force(&name) else {
            return Ok(());
        };
        let buttons = s2sdk::GetClientButtons(player.slot, 0);
        if self_push_suppressed(buttons, s2sdk::GetClientFlags(player.slot)) {
            return Ok(());
        }
        let before = s2sdk::GetClientAbsVelocity(player.slot);
        let angles = s2sdk::GetClientEyeAngles(player.slot);
        let velocity = self_push_velocity(before, angles, force);
        s2sdk::SetClientAbsVelocity(player.slot, &velocity);
        if self.diagnostics {
            s2sdk::PrintToServer(&Str::from_str(&format!(
                "[FunRounds] classic push slot={} weapon={name} force={force} before={before:?} target={velocity:?}",
                player.slot
            )));
        }
        Ok(())
    }

    pub fn apply(&mut self, mode: &Mode, equipment: &[String], current: impl Fn() -> bool) -> Result<(), Fault> {
        if LivePlayer::resolve(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled("player identity changed before applying effects"));
        }
        ensure_equipment_action_current(&current)?;
        self.replace_equipment_inner(equipment, mode, &current)?;
        ensure_equipment_action_current(&current)?;
        let result: Result<(), Fault> = (|| {
            if let Some(max_health) = mode_max_health(mode) {
                write_max_health(self.player, max_health)?;
            }
            s2sdk::SetClientHealth(self.player.slot, i32::from(mode.health));
            s2sdk::SetClientArmor(self.player.slot, i32::from(mode.armor));
            self.player.set_helmet(mode.helmet)?;
            if !speed_matches(s2sdk::GetClientSpeed(self.player.slot), mode.speed) {
                self.speed_written = Some(mode.speed);
                s2sdk::SetClientSpeed(self.player.slot, mode.speed);
            }
            if !speed_matches(s2sdk::GetClientSpeed(self.player.slot), mode.speed) {
                return Err(Fault::mechanism(Mechanism::Classic, "base speed write verification failed"));
            }
            s2sdk::SetClientGravity(self.player.slot, mode.gravity);
            if let (Some(color), Some(rgb)) = (self.color, mode.color) {
                let target = Vec4 {
                    x: f32::from(rgb[0]),
                    y: f32::from(rgb[1]),
                    z: f32::from(rgb[2]),
                    w: color.w,
                };
                s2sdk::SetClientRenderColor(self.player.slot, &target);
                if s2sdk::GetClientRenderColor(self.player.slot) != target {
                    return Err(Fault::mechanism(Mechanism::Color, "render color write verification failed"));
                }
            }
            self.view.apply(self.player, self.round, mode, s2sdk::GetGameTime())?;
            self.weapon_rules.update(self.player, mode)?;
            let gravity = s2sdk::GetClientGravity(self.player.slot);
            if s2sdk::GetClientHealth(self.player.slot) != i32::from(mode.health)
                || s2sdk::GetClientArmor(self.player.slot) != i32::from(mode.armor)
                || self.player.helmet()? != mode.helmet
                || !gravity.is_finite()
                || (gravity - mode.gravity).abs() > 0.0001
            {
                return Err("engine property write verification failed".into());
            }
            Ok(())
        })();
        result?;
        if LivePlayer::resolve(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled("player changed before committing applied effects"));
        }
        commit_equipment_receipt(&mut self.replacement, &current)?;
        Ok(())
    }

    pub fn upgraded_weapon(&self) -> Option<(i32, String)> {
        self.upgraded_weapon.clone()
    }

    pub fn replace_upgrade_weapon(&mut self, target: i32, equipment: &[String], mode: &Mode, current: impl Fn() -> bool) -> Result<i32, Fault> {
        ensure_equipment_action_current(&current)?;
        self.replace_weapon_inner(target, equipment, mode, &current)?;
        ensure_equipment_action_current(&current)?;
        let granted = self
            .replacement
            .as_ref()
            .and_then(equipment::EquipmentReplacement::latest_upgrade_grant)
            .filter(|handle| s2sdk::IsValidEntHandle(*handle))
            .ok_or_else(|| Fault::mechanism(Mechanism::KillLoadout, "upgrade grant was not a new matching weapon"))?;
        if LivePlayer::resolve(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled("player changed before committing upgrade"));
        }
        let classname = s2sdk::GetEntityClassname(granted).to_string();
        commit_equipment_receipt(&mut self.replacement, &current)?;
        self.upgraded_weapon = Some((granted, classname));
        Ok(granted)
    }

    pub fn lifesteal(player: LivePlayer, health_damage: i32, armor_damage: i32, energy_shield: bool) -> Result<(), String> {
        write_max_health(player, funrounds_core::LIFESTEAL_MAX_HEALTH)?;
        let health = s2sdk::GetClientHealth(player.slot);
        if energy_shield {
            let (restored_health, _) = funrounds_core::lifesteal_recovery(health, 0, health_damage, 0);
            s2sdk::SetClientHealth(player.slot, restored_health);
            if s2sdk::GetClientHealth(player.slot) != restored_health {
                return Err("lifesteal health write failed".into());
            }
            return Ok(());
        }
        let armor = s2sdk::GetClientArmor(player.slot);
        let helmet = player.helmet()?;
        let (restored_health, restored_armor) = funrounds_core::lifesteal_recovery(health, armor, health_damage, armor_damage);
        if armor < 1 {
            let _ = s2sdk::GiveNamedItem(player.slot, &Str::from_str("item_assaultsuit"));
            if s2sdk::GetClientArmor(player.slot) < 1 || !player.helmet()? {
                return Err("item_assaultsuit did not enable armor and helmet for lifesteal".into());
            }
        }
        s2sdk::SetClientHealth(player.slot, restored_health);
        s2sdk::SetClientArmor(player.slot, restored_armor);
        let restored_helmet = helmet || armor < 1;
        if player.helmet()? != restored_helmet {
            player.set_helmet(restored_helmet)?;
        }
        if s2sdk::GetClientHealth(player.slot) != restored_health
            || s2sdk::GetClientArmor(player.slot) != restored_armor
            || player.helmet()? != restored_helmet
        {
            return Err("lifesteal property write verification failed".into());
        }
        Ok(())
    }

    pub fn random_damage_heal(player: LivePlayer, amount: i32) -> Result<(), String> {
        if amount <= 0 || LivePlayer::resolve(player.slot) != Some(player) {
            return Err("random-damage heal requires a live matching attacker".into());
        }
        let health = s2sdk::GetClientHealth(player.slot);
        let restored = health.saturating_add(amount).min(funrounds_core::LIFESTEAL_MAX_HEALTH);
        s2sdk::SetClientHealth(player.slot, restored);
        if s2sdk::GetClientHealth(player.slot) != restored {
            return Err("random-damage health write failed".into());
        }
        Ok(())
    }

    pub fn equipment_restored(&self) -> bool {
        self.replacement.is_none()
    }

    pub fn rollback_ready(&self) -> bool {
        self.replacement.as_ref().is_some_and(equipment::EquipmentReplacement::rollback_ready)
    }

    /// Stage equipment cleanup; previous weapons are granted later without holding `RUNTIME`.
    pub fn restore(&mut self, refills: &mut equipment::Refills) -> Result<(), String> {
        let mut errors = Vec::new();
        if let Err(error) = self.weapon_rules.restore() {
            errors.push(error);
        }
        if let Some(translucent) = &mut self.translucent
            && let Err(error) = translucent.restore()
        {
            errors.push(error);
        }
        if let Err(error) = self.restore_failed_equipment(refills, true) {
            errors.push(error);
        }
        if let Err(error) = self.restore_owned_speed() {
            errors.push(error);
        }
        if let Err(error) = self.restore_owned_max_health() {
            errors.push(error);
        }
        if LivePlayer::resolve(self.player.slot) == Some(self.player) {
            s2sdk::SetClientHealth(self.player.slot, self.health);
            s2sdk::SetClientArmor(self.player.slot, self.armor);
            s2sdk::SetClientGravity(self.player.slot, self.gravity);
            if let Err(error) = self.player.set_helmet(self.helmet) {
                errors.push(error);
            }
            if let Some(aim_punch) = self.aim_punch
                && let Err(error) = aim_punch.write(self.player)
            {
                errors.push(error.to_string());
            }
            let gravity = s2sdk::GetClientGravity(self.player.slot);
            if s2sdk::GetClientHealth(self.player.slot) != self.health
                || s2sdk::GetClientArmor(self.player.slot) != self.armor
                || !gravity.is_finite()
                || (gravity - self.gravity).abs() > 0.0001
            {
                errors.push(format!("property restoration failed for slot {}", self.player.slot));
            }
            s2sdk::PrintToServer(&Str::from_str(&format!(
                "[FunRounds] restored slot={} health={} armor={} gravity={} helmet={} color={:?}",
                self.player.slot, self.health, self.armor, self.gravity, self.helmet, self.color
            )));
        }
        if let Err(error) = self.restore_owned_color() {
            errors.push(error);
        }
        if let Err(error) = self.restore_weapon_recoil() {
            errors.push(error);
        }
        if let Err(error) = self.view.restore() {
            errors.push(error);
        }
        if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
    }

    /// Must run outside `RUNTIME`: `GiveNamedItem` can synchronously re-enter game callbacks.
    pub fn restore_previous(&mut self, current: impl Fn() -> bool) -> Result<(), Fault> {
        ensure_equipment_action_current(&current)?;
        let Some(replacement) = self.replacement.as_mut() else { return Ok(()) };
        if !replacement.rollback_ready() {
            return Err(Fault::cancelled("equipment rollback is not staged"));
        }
        if LivePlayer::resolve(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled("player identity changed before equipment rollback"));
        }
        replacement.restore_previous(self.player, &current)?;
        ensure_equipment_action_current(&current)?;
        if replacement.rollback_complete() {
            self.replacement = None;
        }
        Ok(())
    }

    pub fn restore_retired_state(&mut self, refills: &mut equipment::Refills) -> Result<(), String> {
        finish_rollback(
            [
                self.translucent.as_mut().map_or(Ok(()), render::Translucent::restore),
                self.restore_failed_equipment(refills, false),
                self.restore_owned_max_health(),
                self.restore_owned_color(),
                self.restore_weapon_recoil(),
                self.restore_owned_speed(),
                self.view.restore(),
                self.weapon_rules.restore(),
            ]
            .into_iter(),
        )
    }

    fn restore_owned_speed(&self) -> Result<(), String> {
        let Some(speed) = owned_value_to_restore(Some(self.speed), self.player, LivePlayer::resolve_present(self.player.slot)) else {
            return Ok(());
        };
        let Some(speed) = owned_speed_to_restore(speed, self.speed_written, s2sdk::GetClientSpeed(self.player.slot)) else {
            return Ok(());
        };
        s2sdk::SetClientSpeed(self.player.slot, speed);
        if (s2sdk::GetClientSpeed(self.player.slot) - speed).abs() > 0.0001 {
            return Err("classic speed restoration failed".into());
        }
        Ok(())
    }

    fn restore_owned_max_health(&self) -> Result<(), String> {
        let Some(max_health) = owned_value_to_restore(self.max_health, self.player, LivePlayer::resolve_present(self.player.slot)) else {
            return Ok(());
        };
        write_max_health(self.player, max_health)?;
        s2sdk::PrintToServer(&Str::from_str(&format!(
            "[FunRounds] restored max-health slot={} pawn={} value={max_health}",
            self.player.slot, self.player.pawn_handle
        )));
        Ok(())
    }

    fn restore_owned_color(&self) -> Result<(), String> {
        let Some(color) = owned_value_to_restore(self.color, self.player, LivePlayer::resolve_present(self.player.slot)) else {
            return Ok(());
        };
        s2sdk::SetClientRenderColor(self.player.slot, &color);
        if s2sdk::GetClientRenderColor(self.player.slot) == color {
            Ok(())
        } else {
            Err(format!("render color restoration failed for slot {}", self.player.slot))
        }
    }
}

fn speed_matches(actual: f32, expected: f32) -> bool {
    actual.is_finite() && (actual - expected).abs() <= 0.0001
}

fn ensure_equipment_action_current(current: &impl Fn() -> bool) -> Result<(), Fault> {
    if current() {
        Ok(())
    } else {
        Err(Fault::cancelled("equipment action became stale during engine call"))
    }
}

fn commit_equipment_receipt<T>(receipt: &mut Option<T>, current: &impl Fn() -> bool) -> Result<(), Fault> {
    ensure_equipment_action_current(current)?;
    *receipt = None;
    Ok(())
}

fn run_equipment_action<T>(current: &impl Fn() -> bool, action: impl FnOnce() -> T) -> Result<T, Fault> {
    ensure_equipment_action_current(current)?;
    let result = action();
    ensure_equipment_action_current(current)?;
    Ok(result)
}

fn composed_speed(base: f32, health: f32, periodic: f32) -> f32 {
    base * health * periodic
}

fn owned_speed_to_restore(before: f32, written: Option<f32>, current: f32) -> Option<f32> {
    written.filter(|last| speed_matches(current, *last)).map(|_| before)
}

fn mode_max_health(mode: &Mode) -> Option<i32> {
    if mode.lifesteal || mode.random_damage {
        Some(funrounds_core::LIFESTEAL_MAX_HEALTH)
    } else if matches!(mode.self_damage, Some(funrounds_core::SelfDamage::MissPenalty)) && mode.health > 100 {
        Some(i32::from(mode.health))
    } else {
        None
    }
}

fn max_health_pawn(player: LivePlayer) -> Result<usize, String> {
    if LivePlayer::resolve_present(player.slot) != Some(player) {
        return Err("player identity changed before max health access".into());
    }
    if s2sdk::GetSchemaOffset(&Str::from_str(MAX_HEALTH_CLASS), &Str::from_str(MAX_HEALTH_FIELD)) < 0 {
        return Err(format!("{MAX_HEALTH_CLASS}.{MAX_HEALTH_FIELD} schema unavailable"));
    }
    let pawn = s2sdk::EntHandleToEntPointer(player.pawn_handle);
    if pawn == 0 || s2sdk::EntPointerToEntHandle(pawn) != player.pawn_handle {
        return Err("player pawn handle changed before max health access".into());
    }
    Ok(pawn)
}

fn read_max_health(player: LivePlayer) -> Result<i32, String> {
    let pawn = max_health_pawn(player)?;
    let max_health = s2sdk::GetEntSchema2(pawn, &Str::from_str(MAX_HEALTH_CLASS), &Str::from_str(MAX_HEALTH_FIELD), 0);
    i32::try_from(max_health).map_err(|_| format!("invalid {MAX_HEALTH_CLASS}.{MAX_HEALTH_FIELD} value: {max_health}"))
}

fn write_max_health(player: LivePlayer, max_health: i32) -> Result<(), String> {
    let pawn = max_health_pawn(player)?;
    s2sdk::SetEntSchema2(
        pawn,
        &Str::from_str(MAX_HEALTH_CLASS),
        &Str::from_str(MAX_HEALTH_FIELD),
        i64::from(max_health),
        true,
        0,
    );
    if read_max_health(player)? == max_health {
        Ok(())
    } else {
        Err(format!("{MAX_HEALTH_CLASS}.{MAX_HEALTH_FIELD} write verification failed"))
    }
}
fn owned_value_to_restore<T>(value: Option<T>, expected: LivePlayer, current: Option<LivePlayer>) -> Option<T> {
    if current == Some(expected) { value } else { None }
}
fn finish_rollback(steps: impl Iterator<Item = Result<(), String>>) -> Result<(), String> {
    let errors = steps.filter_map(Result::err).collect::<Vec<_>>();
    if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn speed_comparison_skips_equal_values_but_detects_external_changes_and_nan() {
        assert!(super::speed_matches(1.0, 1.0));
        assert!(super::speed_matches(1.00001, 1.0));
        for actual in [0.8, 1.2, f32::NAN, f32::INFINITY] {
            assert!(!super::speed_matches(actual, 1.0));
        }
    }

    #[test]
    fn self_push_preserves_velocity_and_input_rules() {
        use plugify::Vec3;
        assert!(!super::self_push_suppressed(1, 1));
        for (buttons, flags) in [(4, 0), (0x10000, 0), (0, 2), (5, 3)] {
            assert!(super::self_push_suppressed(buttons, flags));
        }
        let before = Vec3 {
            x: 100.0,
            y: 200.0,
            z: 300.0,
        };
        let forward = super::self_push_velocity(before, Vec3 { x: 0.0, y: 0.0, z: 90.0 }, 700.0);
        assert!((forward.x + 670.0).abs() < 0.001 && (forward.y - 60.0).abs() < 0.001 && (forward.z - 90.0).abs() < 0.001);
        let up = super::self_push_velocity(before, Vec3 { x: 90.0, y: 90.0, z: 0.0 }, 700.0);
        assert!((up.x - 30.0).abs() < 0.001 && (up.y - 60.0).abs() < 0.001 && (up.z - 790.0).abs() < 0.001);
    }
    #[test]
    fn max_health_override_comes_from_the_active_effect() {
        let mut mode = funrounds_core::Config::parse(include_str!("../../../config/funrounds.example.toml"))
            .unwrap()
            .modes
            .remove(0);
        assert_eq!(super::mode_max_health(&mode), None);
        mode.lifesteal = true;
        assert_eq!(super::mode_max_health(&mode), Some(funrounds_core::LIFESTEAL_MAX_HEALTH));
        mode.lifesteal = false;
        mode.self_damage = Some(funrounds_core::SelfDamage::MissPenalty);
        assert_eq!(super::mode_max_health(&mode), Some(i32::from(mode.health)));
        mode.health = 100;
        assert_eq!(super::mode_max_health(&mode), None);
    }

    #[test]
    fn owned_color_requires_the_original_controller_and_pawn_handles() {
        let original = crate::game::LivePlayer {
            slot: 2,
            controller_handle: 10,
            pawn_handle: 20,
        };
        let color = super::Vec4 {
            x: 255.0,
            y: 255.0,
            z: 255.0,
            w: 255.0,
        };
        assert_eq!(super::owned_value_to_restore(Some(color), original, Some(original)), Some(color));
        assert_eq!(
            super::owned_value_to_restore(Some(color), original, Some(crate::game::LivePlayer { pawn_handle: 21, ..original }),),
            None,
        );
        assert_eq!(
            super::owned_value_to_restore(
                Some(color),
                original,
                Some(crate::game::LivePlayer {
                    controller_handle: 11,
                    ..original
                }),
            ),
            None,
        );
    }

    #[test]
    fn retired_max_health_restores_only_to_the_original_present_pawn() {
        let original = crate::game::LivePlayer {
            slot: 2,
            controller_handle: 10,
            pawn_handle: 20,
        };
        assert_eq!(super::owned_value_to_restore(Some(100), original, Some(original)), Some(100));
        assert_eq!(
            super::owned_value_to_restore(Some(100), original, Some(crate::game::LivePlayer { pawn_handle: 21, ..original })),
            None,
        );
    }
}
