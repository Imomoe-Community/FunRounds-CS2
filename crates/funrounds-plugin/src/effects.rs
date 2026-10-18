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
use self::recoil::{AimPunchState, WeaponRecoilState};
pub(crate) use recoil::{firearm_event, recoil_schema_probe, recoil_shake_probe};

#[unsafe(no_mangle)]
static mut __s2sdk_GetWeaponItemDefinition: Option<unsafe extern "C" fn(i32) -> u16> = None;

pub fn random_loadout_probe() -> crate::capability::Probe {
    let mut probe = crate::capability::Probe::default();
    probe.require(unsafe { __s2sdk_GetWeaponItemDefinition }.is_some(), "import:GetWeaponItemDefinition");
    probe
}

pub fn classic_probe() -> crate::capability::Probe {
    let mut probe = crate::capability::Probe::default();
    crate::capability::imports!(probe;
        __s2sdk_GetClientSpeed, __s2sdk_SetClientSpeed, __s2sdk_GetClientAbsVelocity,
        __s2sdk_SetClientAbsVelocity, __s2sdk_GetClientEyeAngles, __s2sdk_GetClientButtons,
        __s2sdk_GetClientFlags, __s2sdk_PrintCentreHtmlAll, __s2sdk_PrintToConsoleAll,
        __s2sdk_PrintToChat, __s2sdk_IsFakeClient,
    );
    probe
}

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
    player: LivePlayer,
    diagnostics: bool,
    health: i32,
    armor: i32,
    max_health: Option<i32>,
    helmet: bool,
    gravity: f32,
    speed: f32,
    next_decay: f32,
    color: Option<Vec4>,
    aim_punch: Option<AimPunchState>,
    weapon_recoil: Vec<WeaponRecoilState>,
    replacement: Option<equipment::EquipmentReplacement>,
}

impl Effect {
    pub fn capture(player: LivePlayer, identity: PlayerIdentity, mode: &Mode, diagnostics: bool) -> Result<Self, Fault> {
        Ok(Self {
            identity,
            player,
            diagnostics,
            health: s2sdk::GetClientHealth(player.slot),
            armor: s2sdk::GetClientArmor(player.slot),
            max_health: mode_max_health(mode).map(|_| read_max_health(player)).transpose()?,
            helmet: player.helmet()?,
            gravity: s2sdk::GetClientGravity(player.slot),
            speed: s2sdk::GetClientSpeed(player.slot),
            next_decay: s2sdk::GetGameTime() + 2.0,
            color: mode.blue.then(|| s2sdk::GetClientRenderColor(player.slot)),
            aim_punch: mode.no_spread_recoil.then(|| AimPunchState::capture(player)).transpose()?,
            weapon_recoil: Vec::new(),
            replacement: None,
        })
    }

    pub fn update_classic(&mut self, mode: &Mode, now: f32) -> Result<(), Fault> {
        if LivePlayer::resolve(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled("player identity changed before classic update"));
        }
        let slot = self.player.slot;
        let mut health = s2sdk::GetClientHealth(slot);
        if now >= self.next_decay {
            self.next_decay = now + 2.0;
            let decayed = funrounds_core::decay_health(health, mode.health);
            if decayed != health {
                s2sdk::SetClientHealth(slot, decayed);
                if s2sdk::GetClientHealth(slot) != decayed {
                    return Err("classic health decay write failed".into());
                }
                if self.diagnostics {
                    s2sdk::PrintToServer(&Str::from_str(&format!(
                        "[FunRounds] classic decay slot={slot} before={health} after={decayed}"
                    )));
                }
                health = decayed;
            }
        }
        let (_, speed) = funrounds_core::health_scaling(health, mode.health);
        if !speed_matches(s2sdk::GetClientSpeed(slot), speed) {
            s2sdk::SetClientSpeed(slot, speed);
            if !speed_matches(s2sdk::GetClientSpeed(slot), speed) {
                return Err(Fault::mechanism(Mechanism::Classic, "classic speed write failed"));
            }
        }
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

    pub fn apply(&mut self, mode: &Mode, equipment: &[String]) -> Result<(), Fault> {
        if LivePlayer::resolve(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled("player identity changed before applying effects"));
        }
        self.replace_equipment_inner(equipment, mode)?;
        let result: Result<(), Fault> = (|| {
            if let Some(max_health) = mode_max_health(mode) {
                write_max_health(self.player, max_health)?;
            }
            s2sdk::SetClientHealth(self.player.slot, i32::from(mode.health));
            s2sdk::SetClientArmor(self.player.slot, i32::from(mode.armor));
            self.player.set_helmet(mode.armor > 0)?;
            s2sdk::SetClientGravity(self.player.slot, mode.gravity);
            if let Some(color) = self.color {
                let blue = Vec4 {
                    x: 0.0,
                    y: 0.0,
                    z: 255.0,
                    w: color.w,
                };
                s2sdk::SetClientRenderColor(self.player.slot, &blue);
                if s2sdk::GetClientRenderColor(self.player.slot) != blue {
                    return Err(Fault::mechanism(Mechanism::Blue, "blue render color write verification failed"));
                }
            }
            let gravity = s2sdk::GetClientGravity(self.player.slot);
            if s2sdk::GetClientHealth(self.player.slot) != i32::from(mode.health)
                || s2sdk::GetClientArmor(self.player.slot) != i32::from(mode.armor)
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
        self.replacement = None;
        Ok(())
    }

    pub fn replace_equipment(&mut self, equipment: &[String], mode: &Mode) -> Result<(), Fault> {
        self.replace_equipment_inner(equipment, mode)?;
        if LivePlayer::resolve(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled("player changed before committing loadout"));
        }
        self.replacement = None;
        Ok(())
    }

    pub fn lifesteal(player: LivePlayer, health_damage: i32, armor_damage: i32) -> Result<(), String> {
        write_max_health(player, funrounds_core::LIFESTEAL_MAX_HEALTH)?;
        let health = s2sdk::GetClientHealth(player.slot);
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

    pub fn equipment_restored(&self) -> bool {
        self.replacement.is_none()
    }

    pub fn restore(&mut self, refills: &mut equipment::Refills) -> Result<(), String> {
        let mut errors = Vec::new();
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
        if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
    }

    pub fn restore_retired_state(&mut self, refills: &mut equipment::Refills) -> Result<(), String> {
        finish_rollback(
            [
                self.restore_failed_equipment(refills, false),
                self.restore_owned_max_health(),
                self.restore_owned_color(),
                self.restore_weapon_recoil(),
                self.restore_owned_speed(),
            ]
            .into_iter(),
        )
    }

    fn restore_owned_speed(&self) -> Result<(), String> {
        let Some(speed) = owned_value_to_restore(Some(self.speed), self.player, LivePlayer::resolve_present(self.player.slot)) else {
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
        let Some(color) = color_to_restore(self.color, self.player, LivePlayer::resolve_present(self.player.slot)) else {
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

fn mode_max_health(mode: &Mode) -> Option<i32> {
    if mode.lifesteal {
        Some(funrounds_core::LIFESTEAL_MAX_HEALTH)
    } else if mode.miss_damage && mode.health > 100 {
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
fn color_to_restore(color: Option<Vec4>, expected: LivePlayer, current: Option<LivePlayer>) -> Option<Vec4> {
    owned_value_to_restore(color, expected, current)
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
    fn self_push_preserves_the_legacy_velocity_and_input_rules() {
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
        mode.miss_damage = true;
        assert_eq!(super::mode_max_health(&mode), Some(i32::from(mode.health)));
        mode.health = 100;
        assert_eq!(super::mode_max_health(&mode), None);
    }

    #[test]
    fn retired_color_requires_the_original_controller_and_pawn_handles() {
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
        assert_eq!(super::color_to_restore(Some(color), original, Some(original)), Some(color));
        assert_eq!(
            super::color_to_restore(Some(color), original, Some(crate::game::LivePlayer { pawn_handle: 21, ..original }),),
            None,
        );
        assert_eq!(
            super::color_to_restore(
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
