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
mod ghost;
mod recoil;
mod render;
mod third_person;
pub(crate) mod transmit;
mod view;
mod weapon_rules;
use self::recoil::{AimPunchState, WeaponRecoilState};
pub(crate) use ghost::{
    GhostBreathSource, GhostRuntime, breath_probe as ghost_breath_probe, team_names as ghost_team_names, visibility_probe as ghost_visibility_probe,
};
pub(crate) use recoil::{firearm_event, recoil_schema_probe, recoil_shake_probe};
pub(crate) use render::probe as translucent_probe;
pub(crate) use third_person::{discard_map_targets as discard_third_person_map_targets, probe as third_person_probe};
pub(crate) use view::{crosshair_probe, fov_probe, hide_all_hud_probe, no_scope_probe};
pub(crate) use weapon_rules::{fast_switch_probe, single_shot_probe};

pub(crate) fn c4_removal_probe() -> crate::capability::Probe {
    let mut probe = crate::capability::Probe::default();
    crate::capability::imports!(probe;
        __s2sdk_GetClientWeapons, __s2sdk_RemoveWeapon, __s2sdk_IsValidEntHandle, __s2sdk_GetEntityClassname,
        __s2sdk_RemoveEntity, __s2sdk_GetEntityOwner, __s2sdk_EntHandleToEntPointer, __s2sdk_GetEntSchema2,
    );
    probe.schema("CEntityInstance", "m_pEntity");
    probe.schema("CEntityIdentity", "m_flags");
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
const MOVE_TYPE_CLASS: &str = "CBaseEntity";
const MOVE_TYPE_FIELD: &str = "m_MoveType";
// SourceSDK's MOVETYPE_LADDER is 9; the generated Rust enum has a stale value of 8.
const SOURCE2_MOVE_TYPE_LADDER: i64 = 9;

pub struct Effect {
    pub attributes: funrounds_core::LifeAttributes,
    pub identity: PlayerIdentity,
    pub round: funrounds_core::RoundGeneration,
    player: LivePlayer,
    diagnostics: bool,
    health: i32,
    armor: i32,
    max_health: Option<i32>,
    max_health_written: Option<i32>,
    helmet: bool,
    gravity: f32,
    gravity_written: Option<f32>,
    gravity_relinquished: bool,
    temporary_movement: Option<TemporaryMovement>,
    movement_wait_for_release: u64,
    was_on_ladder: bool,
    speed: f32,
    base_speed: f32,
    speed_written: Option<f32>,
    movement_scale: Option<funrounds_core::MovementScale>,
    next_decay: f32,
    color: Option<OwnedRgb>,
    aim_punch: Option<AimPunchState>,
    weapon_recoil: Vec<WeaponRecoilState>,
    replacement: Option<equipment::EquipmentReplacement>,
    pending_loadout: Option<PendingLoadout>,
    upgraded_weapon: Option<(i32, String, u16)>,
    translucent: Option<render::Translucent>,
    ghost: Option<ghost::GhostEffect>,
    view: view::ViewEffects,
    third_person: third_person::ThirdPerson,
    weapon_rules: weapon_rules::WeaponRules,
}

#[derive(Clone)]
struct PendingLoadout {
    mode: Mode,
    team: s2sdk::CSTeam,
    remaining: Vec<(usize, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct OwnedRgb {
    original: [f32; 3],
    written: [Option<f32>; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TemporaryMovementOwner {
    WallHang,
    Grapple,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TemporaryMovement {
    owner: TemporaryMovementOwner,
    previous_gravity: f32,
    installed_gravity: f32,
}

impl Effect {
    pub fn capture(
        player: LivePlayer,
        identity: PlayerIdentity,
        round: funrounds_core::RoundGeneration,
        mode: &Mode,
        attributes: funrounds_core::LifeAttributes,
        diagnostics: bool,
    ) -> Result<Self, Fault> {
        let color = mode.color.map(|_| {
            let color = s2sdk::GetClientRenderColor(player.slot);
            OwnedRgb {
                original: [color.x, color.y, color.z],
                written: [None; 3],
            }
        });
        Ok(Self {
            attributes,
            identity,
            round,
            player,
            diagnostics,
            health: s2sdk::GetClientHealth(player.slot),
            armor: s2sdk::GetClientArmor(player.slot),
            max_health: mode_max_health(mode, attributes.health)
                .map(|_| read_max_health(player).map_err(Fault::round))
                .transpose()?,
            max_health_written: None,
            helmet: player.helmet().map_err(Fault::round)?,
            gravity: s2sdk::GetClientGravity(player.slot),
            gravity_written: None,
            gravity_relinquished: false,
            temporary_movement: None,
            movement_wait_for_release: 0,
            was_on_ladder: false,
            speed: s2sdk::GetClientSpeed(player.slot),
            base_speed: mode.speed,
            speed_written: None,
            movement_scale: None,
            next_decay: s2sdk::GetGameTime() + mode.health_decay_settings.interval,
            color,
            aim_punch: mode.no_recoil.then(|| AimPunchState::capture(player)).transpose()?,
            weapon_recoil: Vec::new(),
            replacement: None,
            pending_loadout: None,
            upgraded_weapon: None,
            translucent: (mode.translucent || mode.ghost.is_some()).then(|| render::Translucent::new(player, identity, round)),
            ghost: mode.ghost.map(|_| ghost::GhostEffect::default()),
            view: view::ViewEffects::capture(player, identity, round, mode)?,
            third_person: third_person::ThirdPerson::default(),
            weapon_rules: weapon_rules::WeaponRules::default(),
        })
    }

    pub(crate) fn takeover(
        &mut self,
        player: LivePlayer,
        identity: PlayerIdentity,
        round: funrounds_core::RoundGeneration,
        mode: &Mode,
    ) -> Result<(), Fault> {
        if self.round != round || self.identity.pawn_handle != identity.pawn_handle || player.pawn_handle as u32 != identity.pawn_handle {
            return Err(Fault::cancelled("effect pawn changed during bot takeover"));
        }
        // A takeover may replace the controller while retaining this exact pawn.
        // Move the lease to the new current LivePlayer before trying to restore it.
        self.player = player;
        self.identity = identity;
        self.movement_wait_for_release |= (1 << 1) | (1 << 5);
        self.release_temporary_movement()?;
        self.weapon_rules.takeover(player, mode);
        if let Some(translucent) = &mut self.translucent {
            translucent.takeover(player, identity, round)?;
        }
        if let Some(ghost) = &mut self.ghost {
            ghost.reset();
        }
        self.view.takeover(player, identity, round, mode)?;
        Ok(())
    }

    pub fn update_view(&mut self, mode: &Mode, now: f32, round: funrounds_core::RoundGeneration) -> Result<(), Fault> {
        self.view.update(self.player, self.identity, round, mode, now)?;
        self.update_fixed_gravity_after_ladder(mode)?;
        self.weapon_rules.update(self.player, mode)
    }

    pub(crate) fn third_person_needs_update(&self, mode: &Mode) -> bool {
        mode.third_person.is_some() && self.third_person.needs_update(self.player)
    }

    pub(crate) fn update_third_person(&mut self, mode: &Mode, current: impl Fn() -> bool) -> Result<(), Fault> {
        if let Some(settings) = mode.third_person {
            self.third_person.update(self.player, settings, &current)
        } else {
            self.third_person
                .restore()
                .map_err(|error| Fault::mechanism(Mechanism::ThirdPerson, error))
        }
    }

    fn update_fixed_gravity_after_ladder(&mut self, mode: &Mode) -> Result<(), Fault> {
        if let Some(temporary) = self.temporary_movement {
            if read_move_type(self.player).map_err(|error| Fault::mechanism(Mechanism::Classic, error))? == SOURCE2_MOVE_TYPE_LADDER {
                self.was_on_ladder = true;
            }
            let current = s2sdk::GetClientGravity(self.player.slot);
            if speed_matches(current, temporary.installed_gravity) {
                return Ok(());
            }
            // Use the same ownership decision as explicit release: a ladder's
            // native reset must not revoke the mode's gravity for this life.
            self.release_temporary_movement()?;
        }
        if self.gravity_relinquished {
            return Ok(());
        }
        if mode.periodic_gravity.is_some() {
            if read_move_type(self.player).map_err(|error| Fault::mechanism(Mechanism::PeriodicGravity, error))? == SOURCE2_MOVE_TYPE_LADDER {
                self.was_on_ladder = true;
            }
            return Ok(());
        }
        if self.gravity_written.is_none() {
            self.was_on_ladder = false;
            return Ok(());
        }

        let move_type = read_move_type(self.player).map_err(|error| Fault::mechanism(Mechanism::Classic, error))?;
        let current_gravity = s2sdk::GetClientGravity(self.player.slot);
        if move_type == SOURCE2_MOVE_TYPE_LADDER {
            self.was_on_ladder = true;
            if self.gravity_written.is_some_and(|written| !speed_matches(current_gravity, written)) && !speed_matches(current_gravity, 1.0) {
                self.gravity_written = None;
                self.gravity_relinquished = true;
                self.was_on_ladder = false;
            }
            return Ok(());
        }

        let left_ladder = std::mem::replace(&mut self.was_on_ladder, false);
        if self.gravity_written.is_some_and(|written| !speed_matches(current_gravity, written)) {
            if left_ladder && speed_matches(current_gravity, 1.0) {
                self.write_owned_gravity(mode.gravity)
                    .map_err(|error| Fault::mechanism(Mechanism::Classic, error))?;
            } else {
                self.gravity_written = None;
                self.gravity_relinquished = true;
            }
        }
        Ok(())
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

    pub fn update_translucent(&mut self, mode: &Mode, now: f32, round: funrounds_core::RoundGeneration) -> Result<(), Fault> {
        if let Some(settings) = mode
            .ghost
            .filter(|settings| settings.team.matches(s2sdk::GetClientTeam(self.player.slot) as i32))
        {
            let ghost = self.ghost.get_or_insert_with(ghost::GhostEffect::default);
            let alpha = ghost.update(self.player, settings, now);
            if self.translucent.is_none() {
                self.translucent = Some(render::Translucent::new(self.player, self.identity, round));
            }
            if let Some(translucent) = &mut self.translucent {
                translucent.update_alpha(self.identity, round, alpha)?;
            }
            return Ok(());
        }

        self.ghost = None;
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

    pub fn is_fully_invisible_ghost(&self) -> bool {
        self.ghost.as_ref().is_some_and(ghost::GhostEffect::fully_invisible)
    }

    pub fn update_classic(&mut self, mode: &Mode, now: f32, speed_owned: bool) -> Result<(), Fault> {
        if LivePlayer::resolve(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled("player identity changed before classic update"));
        }
        let slot = self.player.slot;
        let mut health = s2sdk::GetClientHealth(slot);
        if mode.health_decay && now >= self.next_decay {
            self.next_decay = now + mode.health_decay_settings.interval;
            let decayed = mode.health_decay_settings.decay(health, self.attributes.health);
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
        if (mode.health_speed
            || mode.periodic_speed.is_some()
            || !speed_matches(self.base_speed, 1.0)
            || mode.ghost.is_some_and(|ghost| ghost.speed_multiplier != 1.0))
            && !speed_owned
        {
            let base_speed = composed_speed(self.base_speed, ghost_speed_multiplier(mode, self.player.slot), 1.0)
                .map_err(|error| Fault::mechanism(Mechanism::Classic, error))?;
            let health_scale = if mode.health_speed {
                mode.health_curve.scaling(health, self.attributes.health).1
            } else {
                1.0
            };
            let periodic_scale = if mode.periodic_speed.is_some() {
                self.movement_scale.and_then(|movement| movement.speed).unwrap_or(1.0)
            } else {
                1.0
            };
            let mechanism = if mode.health_speed {
                Mechanism::HealthSpeed
            } else if mode.periodic_speed.is_some() {
                Mechanism::PeriodicSpeed
            } else {
                Mechanism::Classic
            };
            let speed = composed_speed(base_speed, health_scale, periodic_scale).map_err(|error| Fault::mechanism(mechanism, error))?;
            if !speed_matches(s2sdk::GetClientSpeed(slot), speed) {
                self.speed_written = Some(speed);
                s2sdk::SetClientSpeed(slot, speed);
                if !speed_matches(s2sdk::GetClientSpeed(slot), speed) {
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
        if movement.gravity.is_some() && self.temporary_movement.is_none() && !self.gravity_relinquished {
            let on_ladder = self.was_on_ladder
                && read_move_type(self.player).map_err(|error| Fault::mechanism(Mechanism::PeriodicGravity, error))? == SOURCE2_MOVE_TYPE_LADDER;
            let current_gravity = s2sdk::GetClientGravity(self.player.slot);
            if self.gravity_written.is_some_and(|written| !speed_matches(current_gravity, written))
                && !(self.was_on_ladder && speed_matches(current_gravity, 1.0))
            {
                self.gravity_written = None;
                self.gravity_relinquished = true;
            }
            if !on_ladder {
                self.was_on_ladder = false;
            }
        }
        let periodic_speed = movement
            .speed
            .filter(|_| mode.periodic_speed.is_some() && !speed_owned)
            .map(|periodic_scale| {
                let health_scale = if mode.health_speed {
                    mode.health_curve
                        .scaling(s2sdk::GetClientHealth(self.player.slot), self.attributes.health)
                        .1
                } else {
                    1.0
                };
                let base_speed = composed_speed(self.base_speed, ghost_speed_multiplier(mode, self.player.slot), 1.0)?;
                composed_speed(base_speed, health_scale, periodic_scale)
            })
            .transpose()
            .map_err(|error| Fault::mechanism(Mechanism::PeriodicSpeed, error))?;
        let periodic_gravity = movement
            .gravity
            .filter(|_| mode.periodic_gravity.is_some())
            .map(|scale| composed_gravity(mode.gravity, scale))
            .transpose()
            .map_err(|error| Fault::mechanism(Mechanism::PeriodicGravity, error))?
            .filter(|_| self.temporary_movement.is_none() && !self.gravity_relinquished);
        let speed_matches_current = movement.speed.is_none()
            || speed_owned
            || mode.periodic_speed.is_none()
            || periodic_speed.is_some_and(|speed| speed_matches(s2sdk::GetClientSpeed(self.player.slot), speed));
        let gravity_matches_current = movement.gravity.is_none()
            || mode.periodic_gravity.is_none()
            || self.temporary_movement.is_some()
            || self.gravity_relinquished
            || periodic_gravity.is_some_and(|gravity| speed_matches(s2sdk::GetClientGravity(self.player.slot), gravity));
        if speed_matches_current && gravity_matches_current {
            self.movement_scale = Some(merge_movement_scale(self.movement_scale, movement));
            return Ok(());
        }
        if let Some(speed) = periodic_speed.filter(|_| !speed_owned)
            && !speed_matches(s2sdk::GetClientSpeed(self.player.slot), speed)
        {
            self.speed_written = Some(speed);
            s2sdk::SetClientSpeed(self.player.slot, speed);
        }
        if let Some(gravity) = periodic_gravity
            && let Err(error) = self.write_owned_gravity(gravity)
        {
            return Err(Fault::mechanism(Mechanism::PeriodicGravity, error));
        }
        if periodic_speed.is_some_and(|speed| !speed_owned && !speed_matches(s2sdk::GetClientSpeed(self.player.slot), speed)) {
            return Err(Fault::mechanism(Mechanism::PeriodicSpeed, "periodic speed write failed"));
        }
        if periodic_gravity.is_some_and(|gravity| !speed_matches(s2sdk::GetClientGravity(self.player.slot), gravity)) {
            return Err(Fault::mechanism(Mechanism::PeriodicGravity, "periodic gravity write failed"));
        }
        self.movement_scale = Some(merge_movement_scale(self.movement_scale, movement));
        Ok(())
    }

    pub(crate) fn temporary_movement_owner(&self) -> Option<TemporaryMovementOwner> {
        self.temporary_movement.map(|state| state.owner)
    }

    pub(crate) fn movement_waits_for_release(&self, button_mask: u64) -> bool {
        self.movement_wait_for_release & button_mask != 0
    }

    pub(crate) fn observe_movement_buttons(&mut self, buttons: u64) {
        self.movement_wait_for_release &= buttons;
    }

    pub(crate) fn interrupt_temporary_movement(&mut self) -> Result<(), Fault> {
        let movement_buttons = (1 << 1) | (1 << 5);
        self.stop_temporary_movement(movement_buttons)
    }

    pub(crate) fn stop_temporary_movement(&mut self, wait_for_release: u64) -> Result<(), Fault> {
        self.movement_wait_for_release |= s2sdk::GetClientButtons(self.player.slot, 0) & wait_for_release;
        self.release_temporary_movement()
    }

    pub(crate) fn acquire_temporary_movement(&mut self, owner: TemporaryMovementOwner, gravity: f32) -> Result<(), Fault> {
        let mechanism = match owner {
            TemporaryMovementOwner::WallHang => Mechanism::WallHang,
            TemporaryMovementOwner::Grapple => Mechanism::Grapple,
        };
        if !gravity.is_finite() {
            return Err(Fault::mechanism(mechanism, "temporary movement gravity is not finite"));
        }
        if let Some(existing) = self.temporary_movement {
            if existing.owner == owner {
                return Ok(());
            }
            self.release_temporary_movement()?;
        }
        if self.gravity_relinquished || LivePlayer::resolve_present(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled("temporary movement pawn is stale or gravity ownership was relinquished"));
        }
        let previous = s2sdk::GetClientGravity(self.player.slot);
        if !previous.is_finite() {
            return Err(Fault::mechanism(mechanism, "temporary movement gravity snapshot is not finite"));
        }
        if !speed_matches(previous, gravity) {
            s2sdk::SetClientGravity(self.player.slot, gravity);
        }
        let current = s2sdk::GetClientGravity(self.player.slot);
        if !speed_matches(current, gravity) {
            return Err(Fault::mechanism(mechanism, "temporary movement gravity write failed"));
        }
        self.temporary_movement = Some(TemporaryMovement {
            owner,
            previous_gravity: previous,
            installed_gravity: gravity,
        });
        Ok(())
    }

    pub(crate) fn release_temporary_movement(&mut self) -> Result<(), Fault> {
        let Some(state) = self.temporary_movement else {
            return Ok(());
        };
        if !s2sdk::IsValidEntHandle(self.player.pawn_handle)
            || s2sdk::EntHandleToEntPointer(self.player.pawn_handle) == 0
            || s2sdk::EntPointerToEntHandle(s2sdk::EntHandleToEntPointer(self.player.pawn_handle)) != self.player.pawn_handle
        {
            // The captured entity no longer exists; its old property cannot be reached or affect a new life.
            self.temporary_movement = None;
            self.gravity_written = None;
            return Ok(());
        }
        if LivePlayer::resolve_present(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled(
                "captured movement pawn is still valid but no longer current; snapshot retained",
            ));
        }
        let current = s2sdk::GetClientGravity(self.player.slot);
        if !speed_matches(current, state.installed_gravity) {
            let mechanism = match state.owner {
                TemporaryMovementOwner::WallHang => Mechanism::WallHang,
                TemporaryMovementOwner::Grapple => Mechanism::Grapple,
            };
            let ladder_reset = speed_matches(current, 1.0)
                && (self.was_on_ladder
                    || read_move_type(self.player).map_err(|error| Fault::mechanism(mechanism, error))? == SOURCE2_MOVE_TYPE_LADDER);
            if ladder_reset {
                self.was_on_ladder = true;
                self.movement_wait_for_release |= s2sdk::GetClientButtons(self.player.slot, 0) & ((1 << 1) | (1 << 5));
            } else {
                // A second writer owns gravity now; do not reclaim it this life.
                self.temporary_movement = None;
                self.gravity_written = None;
                self.gravity_relinquished = true;
                self.movement_wait_for_release |= s2sdk::GetClientButtons(self.player.slot, 0) & ((1 << 1) | (1 << 5));
                return Ok(());
            }
        }
        if !speed_matches(current, state.previous_gravity) {
            s2sdk::SetClientGravity(self.player.slot, state.previous_gravity);
        }
        if !speed_matches(s2sdk::GetClientGravity(self.player.slot), state.previous_gravity) {
            let mechanism = match state.owner {
                TemporaryMovementOwner::WallHang => Mechanism::WallHang,
                TemporaryMovementOwner::Grapple => Mechanism::Grapple,
            };
            return Err(Fault::mechanism(mechanism, "temporary movement gravity restoration failed"));
        }
        self.temporary_movement = None;
        Ok(())
    }

    pub(crate) fn update_wall_hang(&mut self, enabled: bool) -> Result<(), Fault> {
        const IN_JUMP: u64 = 1 << 1;
        const ON_GROUND: i32 = 1;
        let active_owner = self.temporary_movement_owner();
        if (!enabled && active_owner.is_none()) || active_owner == Some(TemporaryMovementOwner::Grapple) {
            return Ok(());
        }
        if LivePlayer::resolve_present(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled("player identity changed before wall-hang update"));
        }
        let buttons = s2sdk::GetClientButtons(self.player.slot, 0);
        self.observe_movement_buttons(buttons);
        if !enabled {
            self.movement_wait_for_release |= buttons & IN_JUMP;
            self.release_temporary_movement()?;
            return Ok(());
        }
        let jump_held = wall_hang_jump_held(self.player)?;
        if !crate::funrounds_native::HasPlayerMovementHullSupport() {
            return Err(Fault::mechanism(Mechanism::WallHang, "native movement-hull support became unavailable"));
        }
        if self.movement_waits_for_release(IN_JUMP) {
            return Ok(());
        }
        let walk = s2sdk::GetEntityMoveType(self.player.pawn_handle) == s2sdk::MoveType::Walk;
        let airborne = s2sdk::GetClientFlags(self.player.slot) & ON_GROUND == 0;
        if !walk || !airborne || !jump_held {
            if active_owner == Some(TemporaryMovementOwner::WallHang) {
                if !walk && jump_held {
                    self.movement_wait_for_release |= IN_JUMP;
                }
                self.release_temporary_movement()?;
                if self.diagnostics {
                    s2sdk::PrintToServer(&Str::from_str(&format!(
                        "[FunRounds] wall_hang=released slot={} pawn={} reason={}",
                        self.player.slot,
                        self.player.pawn_handle,
                        if !walk {
                            "movement_type"
                        } else if !airborne {
                            "landed"
                        } else {
                            "jump_released"
                        },
                    )));
                }
            }
            return Ok(());
        }
        let touching_world = wall_hang_world_contact(self.player)?;
        if active_owner == Some(TemporaryMovementOwner::WallHang) {
            if !touching_world {
                self.release_temporary_movement()?;
                if self.diagnostics {
                    s2sdk::PrintToServer(&Str::from_str(&format!(
                        "[FunRounds] wall_hang=released slot={} pawn={} reason=contact_lost qualification_retained=1",
                        self.player.slot, self.player.pawn_handle
                    )));
                }
                return Ok(());
            }
            stop_temporary_player_velocity(self.player, Mechanism::WallHang)?;
        } else if touching_world {
            self.acquire_temporary_movement(TemporaryMovementOwner::WallHang, 0.0)?;
            stop_temporary_player_velocity(self.player, Mechanism::WallHang)?;
            if self.diagnostics {
                s2sdk::PrintToServer(&Str::from_str(&format!(
                    "[FunRounds] wall_hang=latched slot={} pawn={} gravity=0",
                    self.player.slot, self.player.pawn_handle
                )));
            }
        }
        Ok(())
    }

    pub fn self_push(&mut self, player: LivePlayer, weapon: &str) -> Result<(), Fault> {
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
        self.interrupt_temporary_movement()?;
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
        if self.pending_loadout.is_some() {
            return Ok(());
        }
        self.apply_loadout_properties(mode, &current)
    }

    fn apply_loadout_properties(&mut self, mode: &Mode, current: &impl Fn() -> bool) -> Result<(), Fault> {
        ensure_equipment_action_current(&current)?;
        let result: Result<(), Fault> = (|| {
            if let Some(max_health) = mode_max_health(mode, self.attributes.health) {
                self.write_owned_max_health(max_health).map_err(Fault::round)?;
            }
            s2sdk::SetClientHealth(self.player.slot, i32::from(self.attributes.health));
            s2sdk::SetClientArmor(self.player.slot, i32::from(self.attributes.armor));
            self.player.set_helmet(self.attributes.helmet).map_err(Fault::round)?;
            let speed = composed_speed(self.base_speed, ghost_speed_multiplier(mode, self.player.slot), 1.0).map_err(Fault::round)?;
            if !speed_matches(s2sdk::GetClientSpeed(self.player.slot), speed) {
                self.speed_written = Some(speed);
                s2sdk::SetClientSpeed(self.player.slot, speed);
            }
            if !speed_matches(s2sdk::GetClientSpeed(self.player.slot), speed) {
                return Err(Fault::round("base speed write verification failed"));
            }
            self.write_owned_gravity(mode.gravity).map_err(Fault::round)?;
            if let (Some(_), Some(rgb)) = (&self.color, mode.color) {
                self.write_owned_rgb(rgb.map(f32::from))
                    .map_err(|error| Fault::mechanism(Mechanism::Color, error))?;
            }
            self.view.apply(self.player, self.round, mode, s2sdk::GetGameTime())?;
            self.weapon_rules.update(self.player, mode)?;
            if s2sdk::GetClientHealth(self.player.slot) != i32::from(self.attributes.health) {
                return Err(Fault::round("health write verification failed"));
            }
            if s2sdk::GetClientArmor(self.player.slot) != i32::from(self.attributes.armor) {
                return Err(Fault::round("armor write verification failed"));
            }
            if self.player.helmet().map_err(Fault::round)? != self.attributes.helmet {
                return Err(Fault::round("helmet write verification failed"));
            }
            let gravity = s2sdk::GetClientGravity(self.player.slot);
            if !gravity.is_finite() || (gravity - mode.gravity).abs() > 0.0001 {
                return Err(Fault::round("gravity write verification failed"));
            }
            Ok(())
        })();
        result?;
        self.update_third_person(mode, current)?;
        if LivePlayer::resolve(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled("player changed before committing applied effects"));
        }
        commit_equipment_receipt(&mut self.replacement, &current)?;
        Ok(())
    }

    pub fn pending_loadout_confirmation(&self) -> Option<&equipment::PendingEquipmentGrant> {
        self.replacement.as_ref()?.pending_confirmation()
    }

    pub fn has_pending_loadout(&self) -> bool {
        self.pending_loadout.is_some()
    }

    /// Confirm a deferred grant and finish the same loadout transaction on a later game frame.
    pub fn settle_pending_loadout(&mut self, handle: i32, current: impl Fn() -> bool) -> Result<(), Fault> {
        ensure_equipment_action_current(&current)?;
        if LivePlayer::resolve(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled("player identity changed before deferred equipment confirmation"));
        }
        let pending = self
            .pending_loadout_confirmation()
            .filter(|pending| pending.handle == handle)
            .cloned()
            .ok_or_else(|| Fault::cancelled("deferred equipment identity changed before confirmation"))?;
        if s2sdk::GetClientTeam(self.player.slot) as i32 != pending.team {
            return Err(Fault::cancelled("player team changed before deferred equipment confirmation"));
        }
        let current_tick = s2sdk::GetGameTickCount();
        if !equipment::confirmation_due(pending.issued_tick, current_tick) {
            return Err(Fault::cancelled(format!(
                "deferred equipment confirmation did not reach a later game tick: issued={} current={current_tick}",
                pending.issued_tick
            )));
        }

        let valid = s2sdk::IsValidEntHandle(pending.handle);
        let classname = valid.then(|| s2sdk::GetEntityClassname(pending.handle).to_string());
        let item_definition_reader = unsafe { __s2sdk_GetWeaponItemDefinition }.expect("required native ABI checked at startup");
        let item_definition = valid.then(|| unsafe { item_definition_reader(pending.handle) });
        let item_definition_label = equipment::item_definition_diagnostic(item_definition);
        let owner = valid.then(|| s2sdk::GetEntityOwner(pending.handle));
        let inventory = s2sdk::GetClientWeapons(self.player.slot).iter().copied().collect::<Vec<_>>();
        ensure_equipment_action_current(&current)?;
        if LivePlayer::resolve(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled("player identity changed during deferred equipment confirmation"));
        }
        let readback = equipment::EquipmentGrantReadback {
            valid,
            handle: pending.handle,
            owner: owner.unwrap_or(-1),
            inventory: &inventory,
            classname: classname.as_deref().unwrap_or_default(),
            item_definition: item_definition.unwrap_or_default(),
            player_owner: self.player.pawn_handle,
        };
        let settled = self
            .replacement
            .as_mut()
            .is_some_and(|replacement| replacement.settle_pending_confirmation(&pending, readback));
        if !settled {
            let current_weapons = inventory
                .iter()
                .copied()
                .filter(|handle| s2sdk::IsValidEntHandle(*handle))
                .map(|handle| (handle, s2sdk::GetEntityClassname(handle).to_string()))
                .collect::<Vec<_>>();
            return Err(Fault::round(format!(
                "deferred loadout confirmation failed slot={} requested={} handle={} valid={valid} returned_class={} expected_definition={:?} actual_definition={item_definition_label} owner={} pawn={} in_inventory={} current_weapons={current_weapons:?} issued_tick={} current_tick={current_tick}",
                self.player.slot,
                pending.requested,
                pending.handle,
                classname.as_deref().unwrap_or("<invalid-handle>"),
                pending.expected_definition,
                owner.unwrap_or(-1),
                self.player.pawn_handle,
                inventory.contains(&pending.handle),
                pending.issued_tick,
            )));
        }
        s2sdk::PrintToServer(&Str::from_str(&format!(
            "[FunRounds] deferred loadout confirmed slot={} mode={} requested={} handle={} returned_class={} expected_definition={:?} actual_definition={item_definition_label} owner={} inventory=true issued_tick={} current_tick={current_tick}",
            self.player.slot,
            self.pending_loadout
                .as_ref()
                .expect("pending apply accompanies pending confirmation")
                .mode
                .id,
            pending.requested,
            pending.handle,
            classname.as_deref().expect("confirmed item classname"),
            pending.expected_definition,
            owner.expect("confirmed item owner"),
            pending.issued_tick,
        )));
        let Some(work) = self.pending_loadout.take() else {
            return Err(Fault::global("deferred equipment confirmation lost its pending loadout"));
        };
        if work.mode.no_recoil && recoil::is_firearm(&pending.requested) {
            self.capture_weapon_recoil(pending.handle)?;
        }
        self.grant_equipment_items(&work.remaining, &work.mode, None, work.team, &current)?;
        if self.pending_loadout.is_some() {
            return Ok(());
        }
        self.apply_loadout_properties(&work.mode, &current)
    }

    pub fn upgraded_weapon(&self) -> Option<(i32, String, u16)> {
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
        let item_definition_reader = unsafe { __s2sdk_GetWeaponItemDefinition }.expect("required native ABI checked at startup");
        let item_definition = unsafe { item_definition_reader(granted) };
        commit_equipment_receipt(&mut self.replacement, &current)?;
        self.upgraded_weapon = Some((granted, classname, item_definition));
        Ok(granted)
    }

    pub fn lifesteal(&mut self, player: LivePlayer, health_damage: i32, armor_damage: i32, energy_shield: bool) -> Result<(), String> {
        if player != self.player || LivePlayer::resolve_present(player.slot) != Some(player) {
            return Err("player identity changed before lifesteal".into());
        }
        self.write_owned_max_health(funrounds_core::LIFESTEAL_MAX_HEALTH)?;
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

    pub(crate) fn recovery_complete(&self) -> bool {
        self.equipment_restored() && self.third_person.is_clean() && self.temporary_movement.is_none()
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
        if let Err(error) = self.release_temporary_movement() {
            errors.push(error.to_string());
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
        if let Err(error) = self.restore_owned_gravity() {
            errors.push(error);
        }
        if LivePlayer::resolve(self.player.slot) == Some(self.player) {
            s2sdk::SetClientHealth(self.player.slot, self.health);
            s2sdk::SetClientArmor(self.player.slot, self.armor);
            if let Err(error) = self.player.set_helmet(self.helmet) {
                errors.push(error);
            }
            if let Some(aim_punch) = self.aim_punch
                && let Err(error) = aim_punch.write(self.player)
            {
                errors.push(error.to_string());
            }
            if s2sdk::GetClientHealth(self.player.slot) != self.health || s2sdk::GetClientArmor(self.player.slot) != self.armor {
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
        if let Err(error) = self.third_person.restore() {
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
        let movement = self.release_temporary_movement().map_err(|error| error.to_string());
        finish_rollback(
            [
                movement,
                self.translucent.as_mut().map_or(Ok(()), render::Translucent::restore),
                self.restore_failed_equipment(refills, false),
                self.restore_owned_max_health(),
                self.restore_owned_gravity(),
                self.restore_owned_color(),
                self.restore_weapon_recoil(),
                self.restore_owned_speed(),
                self.view.restore(),
                self.third_person.restore(),
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

    fn write_owned_max_health(&mut self, max_health: i32) -> Result<(), String> {
        if self.max_health.is_none() {
            return Err("max-health original value was not captured".into());
        }
        let before = read_max_health(self.player)?;
        let previous = self.max_health_written;
        set_max_health_unverified(self.player, max_health)?;
        self.max_health_written = Some(max_health);
        let after = read_max_health(self.player)?;
        self.max_health_written = owned_integer_after_write(before, after, previous);
        if after != max_health {
            return Err(format!("{MAX_HEALTH_CLASS}.{MAX_HEALTH_FIELD} write verification failed"));
        }
        Ok(())
    }

    fn restore_owned_max_health(&mut self) -> Result<(), String> {
        let Some(original) = owned_value_to_restore(self.max_health, self.player, LivePlayer::resolve_present(self.player.slot)) else {
            return Ok(());
        };
        let Some(written) = self.max_health_written else { return Ok(()) };
        let current = read_max_health(self.player)?;
        if owned_integer_to_restore(original, Some(written), current).is_none() {
            self.max_health_written = None;
            return Ok(());
        }
        write_max_health(self.player, original)?;
        self.max_health_written = None;
        s2sdk::PrintToServer(&Str::from_str(&format!(
            "[FunRounds] restored max-health slot={} pawn={} value={original}",
            self.player.slot, self.player.pawn_handle,
        )));
        Ok(())
    }

    fn write_owned_gravity(&mut self, gravity: f32) -> Result<(), String> {
        if self.gravity_relinquished || self.temporary_movement.is_some() {
            return Ok(());
        }
        let before = s2sdk::GetClientGravity(self.player.slot);
        if !speed_matches(before, gravity) {
            s2sdk::SetClientGravity(self.player.slot, gravity);
        }
        let after = s2sdk::GetClientGravity(self.player.slot);
        self.gravity_written = owned_float_after_write(before, after, self.gravity_written);
        if speed_matches(after, gravity) {
            Ok(())
        } else {
            Err("gravity write verification failed".into())
        }
    }

    fn restore_owned_gravity(&mut self) -> Result<(), String> {
        if self.temporary_movement.is_some() || self.gravity_relinquished {
            return Ok(());
        }
        if LivePlayer::resolve_present(self.player.slot) != Some(self.player) {
            return Ok(());
        }
        let Some(written) = self.gravity_written else { return Ok(()) };
        let current = s2sdk::GetClientGravity(self.player.slot);
        let ladder_reset_still_owned = self.was_on_ladder && speed_matches(current, 1.0);
        if !ladder_reset_still_owned && owned_float_to_restore(self.gravity, Some(written), current).is_none() {
            self.gravity_written = None;
            return Ok(());
        }
        s2sdk::SetClientGravity(self.player.slot, self.gravity);
        if speed_matches(s2sdk::GetClientGravity(self.player.slot), self.gravity) {
            self.gravity_written = None;
            self.was_on_ladder = false;
            Ok(())
        } else {
            Err(format!("gravity restoration failed for slot {}", self.player.slot))
        }
    }

    fn write_owned_rgb(&mut self, rgb: [f32; 3]) -> Result<(), String> {
        let Some(color) = self.color.as_mut() else { return Ok(()) };
        let before = s2sdk::GetClientRenderColor(self.player.slot);
        let before_rgb = [before.x, before.y, before.z];
        for (index, current) in before_rgb.into_iter().enumerate() {
            if color.written[index].is_some_and(|written| !speed_matches(current, written)) {
                color.written[index] = None;
            }
        }
        if !rgb_matches(before_rgb, rgb) {
            s2sdk::SetClientRenderColor(
                self.player.slot,
                &Vec4 {
                    x: rgb[0],
                    y: rgb[1],
                    z: rgb[2],
                    w: before.w,
                },
            );
        }
        let after = s2sdk::GetClientRenderColor(self.player.slot);
        let after_rgb = [after.x, after.y, after.z];
        for index in 0..3 {
            color.written[index] = owned_float_after_write(before_rgb[index], after_rgb[index], color.written[index]);
        }
        if rgb_matches(after_rgb, rgb) {
            Ok(())
        } else {
            Err("render RGB write verification failed".into())
        }
    }

    fn restore_owned_color(&mut self) -> Result<(), String> {
        if LivePlayer::resolve_present(self.player.slot) != Some(self.player) {
            return Ok(());
        }
        let Some(color) = self.color.as_mut() else { return Ok(()) };
        let current = s2sdk::GetClientRenderColor(self.player.slot);
        let current_rgb = [current.x, current.y, current.z];
        let restore_rgb = owned_rgb_to_restore(color.original, color.written, current_rgb);
        let mut target = current_rgb;
        let mut restoring = [false; 3];
        for index in 0..3 {
            if let Some(original) = restore_rgb[index] {
                target[index] = original;
                restoring[index] = true;
            } else {
                if color.written[index].is_some() {
                    color.written[index] = None;
                }
            }
        }
        if !restoring.into_iter().any(|owned| owned) {
            return Ok(());
        }
        s2sdk::SetClientRenderColor(
            self.player.slot,
            &Vec4 {
                x: target[0],
                y: target[1],
                z: target[2],
                w: current.w,
            },
        );
        let after = s2sdk::GetClientRenderColor(self.player.slot);
        let after_rgb = [after.x, after.y, after.z];
        let mut failed = false;
        for index in 0..3 {
            if restoring[index] && speed_matches(after_rgb[index], color.original[index]) {
                color.written[index] = None;
            } else if restoring[index] {
                failed = true;
            }
        }
        if failed {
            Err(format!("render RGB restoration failed for slot {}", self.player.slot))
        } else {
            Ok(())
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

fn ghost_speed_multiplier(mode: &Mode, slot: i32) -> f32 {
    mode.ghost
        .filter(|settings| settings.team.matches(s2sdk::GetClientTeam(slot) as i32))
        .map_or(1.0, |settings| settings.speed_multiplier)
}

fn composed_speed(base: f32, health: f32, periodic: f32) -> Result<f32, &'static str> {
    let speed = base * health * periodic;
    if !speed.is_finite() || speed <= 0.0 {
        return Err("combined movement speed must be finite and positive");
    }
    Ok(speed)
}

fn composed_gravity(base: f32, periodic: f32) -> Result<f32, &'static str> {
    let gravity = base * periodic;
    if !base.is_finite() || base <= 0.0 || !periodic.is_finite() || periodic <= 0.0 || !gravity.is_finite() || gravity <= 0.0 {
        return Err("combined movement gravity must be finite and positive");
    }
    Ok(gravity)
}

fn merge_movement_scale(current: Option<funrounds_core::MovementScale>, update: funrounds_core::MovementScale) -> funrounds_core::MovementScale {
    let current = current.unwrap_or(funrounds_core::MovementScale { speed: None, gravity: None });
    funrounds_core::MovementScale {
        speed: update.speed.or(current.speed),
        gravity: update.gravity.or(current.gravity),
    }
}

fn owned_speed_to_restore(before: f32, written: Option<f32>, current: f32) -> Option<f32> {
    written.filter(|last| speed_matches(current, *last)).map(|_| before)
}

fn owned_float_after_write(before: f32, after: f32, previous: Option<f32>) -> Option<f32> {
    if !speed_matches(before, after) {
        Some(after)
    } else if previous.is_some_and(|written| !speed_matches(after, written)) {
        None
    } else {
        previous
    }
}

fn owned_integer_after_write(before: i32, after: i32, previous: Option<i32>) -> Option<i32> {
    if before != after {
        Some(after)
    } else if previous.is_some_and(|written| after != written) {
        None
    } else {
        previous
    }
}

fn owned_float_to_restore(original: f32, written: Option<f32>, current: f32) -> Option<f32> {
    written.filter(|written| speed_matches(current, *written)).map(|_| original)
}

fn owned_integer_to_restore(original: i32, written: Option<i32>, current: i32) -> Option<i32> {
    written.filter(|written| current == *written).map(|_| original)
}

fn owned_rgb_to_restore(original: [f32; 3], written: [Option<f32>; 3], current: [f32; 3]) -> [Option<f32>; 3] {
    std::array::from_fn(|index| {
        written[index]
            .filter(|written| speed_matches(current[index], *written))
            .map(|_| original[index])
    })
}

fn rgb_matches(actual: [f32; 3], expected: [f32; 3]) -> bool {
    actual.into_iter().zip(expected).all(|(actual, expected)| speed_matches(actual, expected))
}

fn mode_max_health(mode: &Mode, birth_health: u16) -> Option<i32> {
    if mode.lifesteal || mode.random_damage {
        Some(funrounds_core::LIFESTEAL_MAX_HEALTH)
    } else if matches!(mode.self_damage, Some(funrounds_core::SelfDamage::MissPenalty)) && birth_health > 100 {
        Some(i32::from(birth_health))
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

fn read_move_type(player: LivePlayer) -> Result<i64, String> {
    // Cleanup also reads the captured pawn after death, before it is removed.
    if LivePlayer::resolve_present(player.slot) != Some(player) {
        return Err("player identity changed before move-type access".into());
    }
    if s2sdk::GetSchemaOffset(&Str::from_str(MOVE_TYPE_CLASS), &Str::from_str(MOVE_TYPE_FIELD)) < 0 {
        return Err(format!("{MOVE_TYPE_CLASS}.{MOVE_TYPE_FIELD} schema unavailable"));
    }
    let pawn = s2sdk::EntHandleToEntPointer(player.pawn_handle);
    if pawn == 0 || s2sdk::EntPointerToEntHandle(pawn) != player.pawn_handle {
        return Err("player pawn handle changed before move-type access".into());
    }
    Ok(s2sdk::GetEntSchema2(
        pawn,
        &Str::from_str(MOVE_TYPE_CLASS),
        &Str::from_str(MOVE_TYPE_FIELD),
        0,
    ))
}

fn wall_hang_jump_held(player: LivePlayer) -> Result<bool, Fault> {
    const IN_JUMP: i64 = 1 << 1;
    let fail = |reason: &str| Fault::mechanism(Mechanism::WallHang, reason);
    if LivePlayer::resolve_present(player.slot) != Some(player) {
        return Err(Fault::cancelled("player identity changed before jump state read"));
    }
    for (class, field) in [
        ("CBasePlayerPawn", "m_pMovementServices"),
        ("CPlayer_MovementServices", "m_nQueuedButtonDownMask"),
        ("CPlayer_MovementServices", "m_nQueuedButtonChangeMask"),
        ("CPlayer_MovementServices", "m_nButtons"),
        ("CInButtonState", "m_pButtonStates"),
    ] {
        if s2sdk::GetSchemaOffset(&Str::from_str(class), &Str::from_str(field)) < 0 {
            return Err(fail("live player jump-input schema unavailable"));
        }
    }
    let pawn = s2sdk::EntHandleToEntPointer(player.pawn_handle);
    if pawn == 0 || s2sdk::EntPointerToEntHandle(pawn) != player.pawn_handle {
        return Err(Fault::cancelled("player pawn handle changed before jump state read"));
    }
    let movement = s2sdk::GetEntSchema2(pawn, &Str::from_str("CBasePlayerPawn"), &Str::from_str("m_pMovementServices"), 0) as usize;
    if movement == 0 {
        return Err(fail("live player movement services unavailable"));
    }
    let button_state = s2sdk::GetEntSchema2(movement, &Str::from_str("CPlayer_MovementServices"), &Str::from_str("m_nButtons"), 0) as usize;
    if button_state == 0 {
        return Err(fail("live player button state unavailable"));
    }
    let queued_down = s2sdk::GetEntSchema2(
        movement,
        &Str::from_str("CPlayer_MovementServices"),
        &Str::from_str("m_nQueuedButtonDownMask"),
        0,
    );
    let queued_change = s2sdk::GetEntSchema2(
        movement,
        &Str::from_str("CPlayer_MovementServices"),
        &Str::from_str("m_nQueuedButtonChangeMask"),
        0,
    );
    let down = s2sdk::GetEntSchema2(button_state, &Str::from_str("CInButtonState"), &Str::from_str("m_pButtonStates"), 0);
    if LivePlayer::resolve_present(player.slot) != Some(player) {
        return Err(Fault::cancelled("player identity changed while reading jump input"));
    }
    let held = if queued_change & IN_JUMP != 0 {
        queued_down & IN_JUMP != 0
    } else {
        down & IN_JUMP != 0
    };
    Ok(held)
}

fn wall_hang_world_contact(player: LivePlayer) -> Result<bool, Fault> {
    const SWEEP_DISTANCE: f32 = 2.0;
    const INSET: f32 = 0.5;
    const DIAGONAL: f32 = std::f32::consts::FRAC_1_SQRT_2;
    const DIRECTIONS: [(f32, f32); 8] = [
        (1.0, 0.0),
        (DIAGONAL, DIAGONAL),
        (0.0, 1.0),
        (-DIAGONAL, DIAGONAL),
        (-1.0, 0.0),
        (-DIAGONAL, -DIAGONAL),
        (0.0, -1.0),
        (DIAGONAL, -DIAGONAL),
    ];
    let mechanism = Mechanism::WallHang;
    let fail = |reason: &str| Fault::mechanism(mechanism, reason);
    if LivePlayer::resolve_present(player.slot) != Some(player)
        || !s2sdk::IsValidEntHandle(player.pawn_handle)
        || s2sdk::EntHandleToEntPointer(player.pawn_handle) == 0
    {
        return Err(Fault::cancelled("wall-hang pawn identity changed before hull query"));
    }
    for (class, field) in [
        ("CBaseEntity", "m_pCollision"),
        ("CCollisionProperty", "m_vecMins"),
        ("CCollisionProperty", "m_vecMaxs"),
    ] {
        if s2sdk::GetSchemaOffset(&Str::from_str(class), &Str::from_str(field)) < 0 {
            return Err(fail("live pawn collision-hull schema unavailable"));
        }
    }
    let pawn = s2sdk::EntHandleToEntPointer(player.pawn_handle);
    let collision = s2sdk::GetEntSchema2(pawn, &Str::from_str("CBaseEntity"), &Str::from_str("m_pCollision"), 0) as usize;
    if collision == 0 {
        return Err(fail("live pawn collision property unavailable"));
    }
    let entity_mins = s2sdk::GetEntSchemaVector3D2(collision, &Str::from_str("CCollisionProperty"), &Str::from_str("m_vecMins"), 0);
    let entity_maxs = s2sdk::GetEntSchemaVector3D2(collision, &Str::from_str("CCollisionProperty"), &Str::from_str("m_vecMaxs"), 0);
    if ![entity_mins.x, entity_mins.y, entity_mins.z, entity_maxs.x, entity_maxs.y, entity_maxs.z]
        .iter()
        .all(|value| value.is_finite())
        || entity_maxs.x <= entity_mins.x
        || entity_maxs.y <= entity_mins.y
        || entity_maxs.z - entity_mins.z < 24.0
    {
        return Err(fail("live pawn collision hull is malformed"));
    }
    let mins = Vec3 {
        x: entity_mins.x + INSET,
        y: entity_mins.y + INSET,
        z: entity_mins.z + INSET,
    };
    let maxs = Vec3 {
        x: entity_maxs.x - INSET,
        y: entity_maxs.y - INSET,
        z: entity_maxs.z - INSET,
    };
    if maxs.x <= mins.x || maxs.y <= mins.y || maxs.z <= mins.z {
        return Err(fail("live pawn collision hull is too small for the movement trace"));
    }
    let origin = s2sdk::GetClientAbsOrigin(player.slot);
    if ![origin.x, origin.y, origin.z].iter().all(|value| value.is_finite()) {
        return Err(fail("live player origin is not finite"));
    }
    let mut best_gap = f32::INFINITY;
    for (x, y) in DIRECTIONS {
        let start = origin;
        let end = Vec3 {
            x: origin.x + x * SWEEP_DISTANCE,
            y: origin.y + y * SWEEP_DISTANCE,
            z: origin.z,
        };
        let mut position = Vec3::default();
        let mut fraction = f32::NAN;
        let mut hit = false;
        let mut hit_handle = -1;
        let mut start_solid = false;
        let mut normal = Vec3::default();
        if !crate::funrounds_native::TracePlayerMovementHull(
            player.pawn_handle,
            &start,
            &end,
            &mins,
            &maxs,
            &mut position,
            &mut fraction,
            &mut hit,
            &mut hit_handle,
            &mut start_solid,
            &mut normal,
        ) {
            return Err(fail("native player movement-hull query failed"));
        }
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
            return Err(fail("native movement-hull query returned a malformed fraction"));
        }
        if !hit || start_solid || !s2sdk::IsValidEntHandle(hit_handle) {
            continue;
        }
        if hit_handle != s2sdk::EntIndexToEntHandle(0) || s2sdk::GetEntityClassname(hit_handle).as_str() != "worldent" {
            continue;
        }
        let normal_length = (normal.x * normal.x + normal.y * normal.y + normal.z * normal.z).sqrt();
        if !normal_length.is_finite() || !(0.8..=1.2).contains(&normal_length) {
            return Err(fail("native movement-hull query returned a malformed world normal"));
        }
        let gap = fraction * SWEEP_DISTANCE;
        if funrounds_core::wall_hang_contact_eligible(true, true, true, true, start_solid, gap, normal.z) {
            best_gap = best_gap.min(gap);
        }
    }
    Ok(best_gap.is_finite())
}

fn stop_temporary_player_velocity(player: LivePlayer, mechanism: Mechanism) -> Result<(), Fault> {
    if LivePlayer::resolve_present(player.slot) != Some(player) {
        return Err(Fault::cancelled("player identity changed before temporary movement velocity clamp"));
    }
    let zero = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    s2sdk::SetEntityAbsVelocity(player.pawn_handle, &zero);
    let actual = s2sdk::GetEntityAbsVelocity(player.pawn_handle);
    if ![actual.x, actual.y, actual.z].iter().all(|value| value.is_finite()) || actual.x.abs() > 0.1 || actual.y.abs() > 0.1 || actual.z.abs() > 0.1 {
        return Err(Fault::mechanism(mechanism, "temporary movement velocity clamp failed readback"));
    }
    Ok(())
}

fn set_max_health_unverified(player: LivePlayer, max_health: i32) -> Result<(), String> {
    let pawn = max_health_pawn(player)?;
    s2sdk::SetEntSchema2(
        pawn,
        &Str::from_str(MAX_HEALTH_CLASS),
        &Str::from_str(MAX_HEALTH_FIELD),
        i64::from(max_health),
        true,
        0,
    );
    Ok(())
}

fn write_max_health(player: LivePlayer, max_health: i32) -> Result<(), String> {
    set_max_health_unverified(player, max_health)?;
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
        assert_eq!(super::mode_max_health(&mode, 120), None);
        mode.lifesteal = true;
        assert_eq!(super::mode_max_health(&mode, 120), Some(funrounds_core::LIFESTEAL_MAX_HEALTH));
        mode.lifesteal = false;
        mode.self_damage = Some(funrounds_core::SelfDamage::MissPenalty);
        assert_eq!(super::mode_max_health(&mode, 120), Some(120));
        mode.health = funrounds_core::AttributeValue::Fixed(100);
        assert_eq!(super::mode_max_health(&mode, 100), None);
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
