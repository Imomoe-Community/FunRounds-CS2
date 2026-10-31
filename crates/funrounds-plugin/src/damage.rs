// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::s2sdk;

pub type DamagePreCallback = crate::funrounds_native::DamagePreCallback;
pub const DMG_BULLET: u32 = 1 << 1;
pub const DMG_FALL: u32 = 1 << 5;
pub const DMG_BURN: u32 = 1 << 3;
pub const DMG_SLASH: u32 = 1 << 2;
pub const DMG_BLAST: u32 = 1 << 6;
pub const DMG_HEADSHOT: u32 = 1 << 25;
const DELAYED_DAMAGE_SECONDS: f32 = 3.0;

#[derive(Clone)]
struct InternalDamage {
    victim: i32,
    attacker: i32,
    inflictor: i32,
    weapon: i32,
    victim_slot: i32,
    attacker_slot: i32,
    headshot: bool,
    check_protection: bool,
    suppress_lifesteal: bool,
    random_heal: Option<crate::runtime::RandomHealReceipt>,
    frame_token: Option<u64>,
    death_weapon: Option<String>,
}

impl InternalDamage {
    fn new(context: InternalDamageContext) -> Self {
        Self {
            victim: context.victim,
            attacker: context.attacker,
            inflictor: context.inflictor,
            weapon: context.weapon,
            victim_slot: context.victim_slot,
            attacker_slot: context.attacker_slot,
            headshot: context.headshot,
            check_protection: context.check_protection,
            suppress_lifesteal: context.suppress_lifesteal,
            random_heal: context.random_heal,
            frame_token: None,
            death_weapon: context.death_weapon,
        }
    }
}

pub(crate) struct InternalDamageContext {
    pub victim: i32,
    pub attacker: i32,
    pub inflictor: i32,
    pub weapon: i32,
    pub victim_slot: i32,
    pub attacker_slot: i32,
    pub headshot: bool,
    // Replays already checked at settlement leave this false; suicide blast
    // checks in the native pre-hook, while its forced finish bypasses protection.
    pub check_protection: bool,
    pub suppress_lifesteal: bool,
    pub random_heal: Option<crate::runtime::RandomHealReceipt>,
    pub death_weapon: Option<String>,
}

std::thread_local! {
    static INTERNAL_DAMAGE: std::cell::RefCell<Option<InternalDamage>> = const { std::cell::RefCell::new(None) };
}

struct InternalDamageGuard(Option<InternalDamage>);

impl InternalDamageGuard {
    fn enter(damage: InternalDamage) -> Self {
        Self(INTERNAL_DAMAGE.with(|active| active.replace(Some(damage))))
    }
}

pub(crate) fn with_internal_damage_context<R>(context: InternalDamageContext, action: impl FnOnce() -> R) -> R {
    let _scope = InternalDamageGuard::enter(InternalDamage::new(context));
    action()
}

impl Drop for InternalDamageGuard {
    fn drop(&mut self) {
        INTERNAL_DAMAGE.with(|active| active.replace(self.0.take()));
    }
}

fn internal_damage(damage: &Damage) -> Option<(bool, bool)> {
    if !damage.skip_plugin_rules {
        return None;
    }
    INTERNAL_DAMAGE.with(|active| {
        active.borrow_mut().as_mut().and_then(|expected| {
            let matches = damage.victim == expected.victim
                && damage.attacker == expected.attacker
                && damage.inflictor == expected.inflictor
                && damage.weapon == expected.weapon;
            if matches && (expected.suppress_lifesteal || expected.death_weapon.is_some() || expected.random_heal.is_some()) {
                expected.frame_token = current_damage_frame_token();
            }
            matches.then_some((expected.headshot, expected.check_protection))
        })
    })
}

pub(crate) fn internal_damage_attacker_slot(damage: &Damage) -> Option<i32> {
    if !damage.skip_plugin_rules {
        return None;
    }
    INTERNAL_DAMAGE.with(|active| {
        active.borrow().as_ref().and_then(|expected| {
            (damage.victim == expected.victim
                && damage.attacker == expected.attacker
                && damage.inflictor == expected.inflictor
                && damage.weapon == expected.weapon)
                .then_some(expected.attacker_slot)
        })
    })
}

pub fn internal_random_heal_hurt(victim: i32, attacker: i32) -> Option<crate::runtime::RandomHealReceipt> {
    let token = current_damage_frame_token().filter(|token| *token != 0)?;
    INTERNAL_DAMAGE.with(|active| {
        let mut active = active.borrow_mut();
        let expected = active.as_mut()?;
        if expected.victim_slot == victim && expected.attacker_slot == attacker && expected.frame_token == Some(token) {
            expected.random_heal.take().map(|mut receipt| {
                receipt.frame_token = token;
                receipt
            })
        } else {
            None
        }
    })
}

pub fn internal_damage_hurt(victim: i32, attacker: i32) -> Result<bool, crate::fault::Fault> {
    INTERNAL_DAMAGE.with(|active| {
        let active = active.borrow();
        let Some(expected) = active
            .as_ref()
            .filter(|expected| expected.suppress_lifesteal && expected.victim_slot == victim && expected.attacker_slot == attacker)
        else {
            return Ok(false);
        };
        let Some(token) = current_damage_frame_token().filter(|token| *token != 0) else {
            return Err(crate::fault::Fault::mechanism(
                crate::fault::Mechanism::ReflectionLifestealFrame,
                "damage frame token unavailable during reflected player_hurt",
            ));
        };
        let Some(expected_token) = expected.frame_token else {
            return Err(crate::fault::Fault::mechanism(
                crate::fault::Mechanism::ReflectionLifestealFrame,
                "reflected damage did not register its native frame",
            ));
        };
        Ok(expected_token == token)
    })
}

pub fn internal_suicide_hurt(victim: i32, attacker: i32) -> bool {
    let token = current_damage_frame_token().filter(|token| *token != 0);
    INTERNAL_DAMAGE.with(|active| {
        active.borrow().as_ref().is_some_and(|expected| {
            expected.death_weapon.as_deref() == Some("c4")
                && expected.victim_slot == victim
                && expected.attacker_slot == attacker
                && token.is_some()
                && expected.frame_token == token
        })
    })
}

pub fn internal_damage_death_weapon(victim: i32, attacker: i32) -> Option<String> {
    let token = current_damage_frame_token().filter(|token| *token != 0)?;
    INTERNAL_DAMAGE.with(|active| {
        active.borrow().as_ref().and_then(|expected| {
            (expected.frame_token == Some(token) && expected.victim_slot == victim && expected.attacker_slot == attacker)
                .then(|| expected.death_weapon.clone())
                .flatten()
        })
    })
}

pub fn current_damage_frame_token() -> Option<u64> {
    unsafe { crate::funrounds_native::__funrounds_native_GetCurrentDamageFrameToken }.map(|query| unsafe { query() })
}

pub fn damage_frame_token_available() -> bool {
    unsafe { crate::funrounds_native::__funrounds_native_GetCurrentDamageFrameToken }.is_some()
}

// Internal native replays synchronously re-enter the damage hook; exact handles keep unrelated damage in scope.
pub fn apply_internal_damage(damage: &Damage, mark_headshot: &mut bool, protected: impl FnOnce() -> bool) -> Option<i32> {
    let (headshot, check_protection) = internal_damage(damage)?;
    if check_protection && protected() {
        return Some(1);
    }
    *mark_headshot = headshot;
    Some(0)
}

pub struct Damage {
    pub victim: i32,
    pub attacker: i32,
    pub inflictor: i32,
    pub weapon_kind: i32,
    pub hit_group: i32,
    pub traced_head: bool,
    pub amount: f32,
    pub damage_type: u32,
    pub weapon: i32,
    pub weapon_class: Option<String>,
    pub skip_plugin_rules: bool,
}

impl Damage {
    pub fn is_head_hit(&self, player_attributed: bool) -> bool {
        player_attributed && matches!(headshot_rule(true, self.weapon_kind, self.hit_group, self.traced_head), (0, _, true))
    }
    pub fn headshot_rule(&self, player_attributed: bool, classic: bool) -> (i32, f32, bool) {
        if classic && self.damage_type & DMG_FALL != 0 {
            return (0, 1.0, false);
        }
        // Engine inferno hits and owned entityflame afterburn ticks are not
        // headshots. Their attacker may be unattributed, which otherwise falls
        // through the generic world-damage exception below.
        if self.damage_type & DMG_BURN != 0 {
            return (1, 1.0, false);
        }
        headshot_rule(player_attributed, self.weapon_kind, self.hit_group, self.traced_head)
    }
}

pub fn capture_weapon_class(weapon: i32, inflictor: i32, weapon_kind: i32) -> Option<String> {
    for handle in [weapon, inflictor] {
        if handle != -1 && s2sdk::IsValidEntHandle(handle) {
            let classname = s2sdk::GetEntityClassname(handle).to_string();
            if classname.starts_with("weapon_") || classname.ends_with("_projectile") {
                return Some(classname);
            }
        }
    }
    match weapon_kind {
        2 => Some("weapon_knife".into()),
        3 => Some("weapon_taser".into()),
        _ => None,
    }
}

pub fn weapon_scale_class(classname: &str) -> &str {
    classname.strip_suffix("_projectile").unwrap_or(classname)
}

fn normalize_event_weapon(classname: &str) -> Option<String> {
    let classname = classname.strip_prefix("weapon_").unwrap_or(classname);
    let classname = classname.strip_suffix("_projectile").unwrap_or(classname);
    let classname = if classname == "knife_t" { "knife" } else { classname };
    (!classname.is_empty() && !classname.starts_with("cs_")).then(|| classname.to_owned())
}

pub fn event_weapon(classname: Option<&str>) -> Option<String> {
    classname.and_then(normalize_event_weapon)
}

pub struct Bridge {
    callback: DamagePreCallback,
}

impl Bridge {
    pub fn subscribe(callback: DamagePreCallback) -> Option<Self> {
        if !crate::funrounds_native::HasPlayerTakeDamagePre() {
            return None;
        }
        crate::funrounds_native::HookPlayerTakeDamagePre(callback).then_some(Self { callback })
    }

    pub fn unsubscribe(&self) -> bool {
        crate::funrounds_native::UnhookPlayerTakeDamagePre(self.callback)
    }
}

pub struct EntityBridge {
    callback: crate::funrounds_native::EntityDamagePreCallback,
}

impl EntityBridge {
    pub fn subscribe(callback: crate::funrounds_native::EntityDamagePreCallback) -> (Option<Self>, bool) {
        // These three imports are optional additions to the native ABI.
        // They are optional for existing installations, so check the whole new
        // surface before calling any generated wrapper (which expects imports).
        let imports_available = unsafe {
            std::ptr::addr_of!(crate::funrounds_native::__funrounds_native_HasEntityTakeDamagePre)
                .read()
                .is_some()
                && std::ptr::addr_of!(crate::funrounds_native::__funrounds_native_HookEntityTakeDamagePre)
                    .read()
                    .is_some()
                && std::ptr::addr_of!(crate::funrounds_native::__funrounds_native_UnhookEntityTakeDamagePre)
                    .read()
                    .is_some()
        };
        if !imports_available {
            return (None, false);
        }
        if !crate::funrounds_native::HasEntityTakeDamagePre() {
            return (None, false);
        }
        if crate::funrounds_native::HookEntityTakeDamagePre(callback) {
            (Some(Self { callback }), true)
        } else {
            // Registration may have left a partial shared detour; only a confirmed detach
            // permits a later start attempt.
            let detached = crate::funrounds_native::UnhookEntityTakeDamagePre(callback);
            ((!detached).then_some(Self { callback }), false)
        }
    }

    pub fn unsubscribe(&self) -> bool {
        crate::funrounds_native::UnhookEntityTakeDamagePre(self.callback)
    }
}

pub fn reflection_available() -> bool {
    unsafe { crate::funrounds_native::__funrounds_native_TakeEntityDamageWithWeapon }.is_some()
}

pub(crate) struct NativeDamageCall<'a> {
    pub victim: i32,
    pub inflictor: i32,
    pub attacker: i32,
    pub force: &'a plugify::Vec3,
    pub position: &'a plugify::Vec3,
    pub amount: f32,
    pub damage_type: u32,
    pub weapon: i32,
    pub skip_plugin_rules: bool,
}

pub fn apply_native_damage(call: NativeDamageCall<'_>, mechanism: crate::fault::Mechanism, failure: &str) -> Result<i32, crate::fault::Fault> {
    if !reflection_available() {
        return Err(crate::fault::Fault::mechanism(
            mechanism,
            format!("{failure}: native damage API unavailable"),
        ));
    }
    let result = crate::funrounds_native::TakeEntityDamageWithWeapon(
        call.victim,
        call.inflictor,
        call.attacker,
        call.force,
        call.position,
        call.amount,
        call.damage_type as i32,
        call.weapon,
        call.skip_plugin_rules,
    );
    native_damage_result(result, mechanism, failure)
}

fn native_damage_result(result: i32, mechanism: crate::fault::Mechanism, failure: &str) -> Result<i32, crate::fault::Fault> {
    if result < 0 {
        Err(crate::fault::Fault::mechanism(mechanism, failure))
    } else {
        Ok(result)
    }
}

pub fn finish_self_damage(pawn: i32) -> Result<i32, crate::fault::Fault> {
    let zero = plugify::Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    with_internal_damage_context(
        InternalDamageContext {
            victim: pawn,
            attacker: pawn,
            inflictor: pawn,
            weapon: -1,
            victim_slot: -1,
            attacker_slot: -1,
            headshot: false,
            check_protection: false,
            suppress_lifesteal: true,
            random_heal: None,
            death_weapon: None,
        },
        || {
            apply_native_damage(
                NativeDamageCall {
                    victim: pawn,
                    inflictor: pawn,
                    attacker: pawn,
                    force: &zero,
                    position: &zero,
                    amount: 1.0,
                    damage_type: 0x0200_0000,
                    weapon: -1,
                    skip_plugin_rules: true,
                },
                crate::fault::Mechanism::NativeSelfDamage,
                "native self-damage finish rejected after identity validation",
            )
        },
    )
}

pub fn delayed_damage_eligible(target_slot: i32, source_slot: i32, damage_type: u32, amount: f32) -> bool {
    target_slot != source_slot && damage_type & DMG_FALL == 0 && amount.is_finite() && amount > 0.0
}

#[derive(Clone)]
pub struct DelayedDamage {
    pub target: funrounds_core::PlayerTask,
    pub source: funrounds_core::PlayerTask,
    pub target_live: crate::game::LivePlayer,
    pub source_live: crate::game::LivePlayer,
    pub inflictor: i32,
    pub weapon: i32,
    pub weapon_class: Option<String>,
    pub amount: f32,
    pub damage_type: u32,
    pub headshot: bool,
    pub random_heal: i32,
    due_at: f32,
}

#[derive(Clone, Copy)]
pub struct DamageOutcome {
    pub amount: f32,
    pub headshot: bool,
}

fn native_handles(inflictor: i32, source_pawn: i32, weapon: i32, mut is_valid: impl FnMut(i32) -> bool) -> Option<(i32, i32)> {
    let inflictor = if inflictor != -1 && is_valid(inflictor) {
        inflictor
    } else if source_pawn != -1 && is_valid(source_pawn) {
        source_pawn
    } else {
        return None;
    };
    let weapon = if weapon != -1 && is_valid(weapon) { weapon } else { -1 };
    Some((inflictor, weapon))
}

impl DelayedDamage {
    pub fn new(
        target: funrounds_core::PlayerTask,
        source: funrounds_core::PlayerTask,
        target_live: crate::game::LivePlayer,
        source_live: crate::game::LivePlayer,
        damage: &Damage,
        outcome: DamageOutcome,
        now: f32,
    ) -> Self {
        Self {
            target,
            source,
            target_live,
            source_live,
            inflictor: damage.inflictor,
            weapon: damage.weapon,
            weapon_class: damage.weapon_class.clone(),
            amount: outcome.amount,
            damage_type: damage.damage_type,
            headshot: outcome.headshot,
            random_heal: 0,
            due_at: now + DELAYED_DAMAGE_SECONDS,
        }
    }

    pub fn native_handles(&self, is_valid: impl FnMut(i32) -> bool) -> Option<(i32, i32)> {
        native_handles(self.inflictor, self.source_live.pawn_handle, self.weapon, is_valid)
    }

    pub fn is_due(&self, now: f32) -> bool {
        now >= self.due_at
    }

    pub fn set_random_heal(&mut self, amount: i32) {
        self.random_heal = amount.max(0);
    }

    pub fn validates_players(
        &self,
        round: funrounds_core::RoundGeneration,
        players: &funrounds_core::PlayerTracker,
        target_live: Option<crate::game::LivePlayer>,
        source_live: Option<crate::game::LivePlayer>,
    ) -> bool {
        target_live == Some(self.target_live)
            && source_live == Some(self.source_live)
            && players.validates(self.target, round, self.target_live.pawn_handle as u32)
            && players.validates_damage_source(self.source, round, self.source_live.pawn_handle as u32)
    }

    pub fn execute(self, is_valid: impl FnMut(i32) -> bool) -> Result<(), crate::fault::Fault> {
        let Some((inflictor, weapon)) = self.native_handles(is_valid) else {
            return Err(crate::fault::Fault::cancelled(
                "delayed damage has no valid native inflictor after identity validation",
            ));
        };
        let result = with_internal_damage_context(
            InternalDamageContext {
                victim: self.target_live.pawn_handle,
                attacker: self.source_live.pawn_handle,
                inflictor,
                weapon,
                victim_slot: self.target.player.slot,
                attacker_slot: self.source.player.slot,
                headshot: self.headshot,
                check_protection: false,
                suppress_lifesteal: false,
                random_heal: (self.random_heal > 0).then_some(crate::runtime::RandomHealReceipt {
                    target: self.target,
                    source: self.source,
                    target_live: self.target_live,
                    source_live: self.source_live,
                    frame_token: 0,
                    amount: self.random_heal,
                }),
                death_weapon: event_weapon(self.weapon_class.as_deref()),
            },
            || {
                apply_native_damage(
                    NativeDamageCall {
                        victim: self.target_live.pawn_handle,
                        inflictor,
                        attacker: self.source_live.pawn_handle,
                        force: &plugify::Vec3 { x: 0.0, y: 0.0, z: 0.0 },
                        position: &plugify::Vec3 { x: 0.0, y: 0.0, z: 0.0 },
                        amount: self.amount,
                        damage_type: self.damage_type,
                        weapon,
                        skip_plugin_rules: true,
                    },
                    crate::fault::Mechanism::DelayedDamage,
                    "native delayed damage rejected after identity validation",
                )
            },
        );
        result
            .map(|_| ())
            .map_err(|fault| fault.context(format!("target slot={} source slot={}", self.target.player.slot, self.source.player.slot)))
    }
}

#[derive(Clone)]
pub struct Reflection {
    pub target: funrounds_core::PlayerTask,
    pub source: funrounds_core::PlayerTask,
    pub target_live: crate::game::LivePlayer,
    pub source_live: crate::game::LivePlayer,
    pub inflictor: i32,
    pub weapon: i32,
    pub weapon_class: Option<String>,
    pub amount: f32,
    pub damage_type: u32,
    pub tick: i32,
    pub suppress_lifesteal: bool,
}

impl Reflection {
    pub fn native_handles(&self, is_valid: impl FnMut(i32) -> bool) -> Option<(i32, i32)> {
        native_handles(self.inflictor, self.source_live.pawn_handle, self.weapon, is_valid)
    }

    pub fn execute(&self, is_valid: impl FnMut(i32) -> bool) -> Result<(), crate::fault::Fault> {
        let Some((inflictor, weapon)) = self.native_handles(is_valid) else {
            return Err(crate::fault::Fault::cancelled(
                "reflection has no valid native inflictor after identity validation",
            ));
        };
        let result = with_internal_damage_context(
            InternalDamageContext {
                victim: self.target_live.pawn_handle,
                attacker: self.source_live.pawn_handle,
                inflictor,
                weapon,
                victim_slot: self.target.player.slot,
                attacker_slot: self.source.player.slot,
                headshot: false,
                check_protection: false,
                suppress_lifesteal: self.suppress_lifesteal,
                random_heal: None,
                death_weapon: event_weapon(self.weapon_class.as_deref()),
            },
            || {
                apply_native_damage(
                    NativeDamageCall {
                        victim: self.target_live.pawn_handle,
                        inflictor,
                        attacker: self.source_live.pawn_handle,
                        force: &plugify::Vec3 { x: 0.0, y: 0.0, z: 0.0 },
                        position: &plugify::Vec3 { x: 0.0, y: 0.0, z: 0.0 },
                        amount: self.amount,
                        damage_type: self.damage_type,
                        weapon,
                        skip_plugin_rules: true,
                    },
                    crate::fault::Mechanism::Reflection,
                    "native reflected damage rejected after identity validation",
                )
            },
        );
        result.map(|_| ())
    }
}

pub fn headshot_rule(player_attributed: bool, weapon_kind: i32, hit_group: i32, traced_head: bool) -> (i32, f32, bool) {
    if !player_attributed {
        return (0, 1.0, false);
    }
    match weapon_kind {
        1 if hit_group == 1 => (0, 1.0, true),
        2 | 3 if traced_head => (0, 1.5, true),
        _ => (1, 1.0, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn anti_headshot_blocks_only_firearm_heads_and_special_knife_zeus_heads() {
        let mut damage = Damage {
            victim: 42,
            attacker: 43,
            inflictor: 43,
            weapon_kind: 1,
            hit_group: 1,
            traced_head: false,
            amount: 50.0,
            damage_type: 2,
            weapon: -1,
            weapon_class: None,
            skip_plugin_rules: false,
        };
        assert!(damage.is_head_hit(true));
        assert!(!damage.is_head_hit(false));
        damage.hit_group = 2;
        assert!(!damage.is_head_hit(true));
        for kind in [2, 3] {
            damage.weapon_kind = kind;
            damage.hit_group = 1;
            assert!(!damage.is_head_hit(true));
            damage.traced_head = true;
            assert!(damage.is_head_hit(true));
            damage.traced_head = false;
        }
        damage.weapon_kind = 0;
        damage.hit_group = 1;
        damage.traced_head = true;
        assert!(!damage.is_head_hit(true));
    }

    #[test]
    fn fall_damage_keeps_its_standard_scaling_and_headshot_rules() {
        let mut damage = Damage {
            victim: 42,
            attacker: 42,
            inflictor: 42,
            weapon_kind: 0,
            hit_group: -1,
            traced_head: false,
            amount: 450.0,
            damage_type: 32,
            weapon: -1,
            weapon_class: None,
            skip_plugin_rules: false,
        };
        let (action, special, mark) = damage.headshot_rule(true, true);
        assert_eq!((action, special, mark), (0, 1.0, false));
        assert_eq!(
            funrounds_core::HealthCurve::default()
                .damage_scale(Some((450, 100)), true, true, false)
                .map(|common| common * special),
            Some(1.0)
        );
        assert_eq!(damage.headshot_rule(true, false).0, 1);
        damage.damage_type = 2;
        damage.weapon_kind = 1;
        damage.hit_group = 2;
        assert_eq!(damage.headshot_rule(true, true).0, 1);
        damage.hit_group = 1;
        assert_eq!(damage.headshot_rule(true, true), (0, 1.0, true));
    }

    static HOOK_CALLS: AtomicU32 = AtomicU32::new(0);
    static UNHOOK_CALLS: AtomicU32 = AtomicU32::new(0);

    unsafe extern "C" fn callback(
        _: i32,
        _: i32,
        _: i32,
        _: i32,
        _: i32,
        _: bool,
        _: f32,
        _: u32,
        _: &mut f32,
        _: &mut bool,
        _: i32,
        _: bool,
    ) -> i32 {
        0
    }
    unsafe extern "C" fn native_available() -> bool {
        true
    }
    unsafe extern "C" fn native_unavailable() -> bool {
        false
    }
    unsafe extern "C" fn hook(_: DamagePreCallback) -> bool {
        HOOK_CALLS.fetch_add(1, Ordering::Relaxed);
        true
    }
    unsafe extern "C" fn reject(_: DamagePreCallback) -> bool {
        HOOK_CALLS.fetch_add(1, Ordering::Relaxed);
        false
    }
    unsafe extern "C" fn unhook(_: DamagePreCallback) -> bool {
        UNHOOK_CALLS.fetch_add(1, Ordering::Relaxed);
        true
    }

    #[test]
    fn bridge_requires_native_prerequisites_and_releases_its_callback() {
        use crate::funrounds_native as native;
        HOOK_CALLS.store(0, Ordering::Relaxed);
        UNHOOK_CALLS.store(0, Ordering::Relaxed);
        unsafe {
            native::__funrounds_native_HasPlayerTakeDamagePre = Some(native_available);
            native::__funrounds_native_HookPlayerTakeDamagePre = Some(hook);
            native::__funrounds_native_UnhookPlayerTakeDamagePre = Some(unhook);
        }
        let bridge = Bridge::subscribe(callback).unwrap();
        assert_eq!(HOOK_CALLS.load(Ordering::Relaxed), 1);
        assert!(bridge.unsubscribe());
        assert_eq!(UNHOOK_CALLS.load(Ordering::Relaxed), 1);

        unsafe {
            native::__funrounds_native_HasPlayerTakeDamagePre = Some(native_unavailable);
        }
        assert!(Bridge::subscribe(callback).is_none());
        unsafe {
            native::__funrounds_native_HasPlayerTakeDamagePre = Some(native_available);
            native::__funrounds_native_HookPlayerTakeDamagePre = Some(reject);
        }
        assert!(Bridge::subscribe(callback).is_none());
        assert_eq!(HOOK_CALLS.load(Ordering::Relaxed), 2);
        unsafe {
            native::__funrounds_native_HasPlayerTakeDamagePre = None;
            native::__funrounds_native_HookPlayerTakeDamagePre = None;
            native::__funrounds_native_UnhookPlayerTakeDamagePre = None;
        }
    }

    #[test]
    fn complete_special_headshot_rules_keep_world_damage_and_block_attributed_non_heads() {
        use super::headshot_rule;
        assert_eq!(headshot_rule(false, 0, 0, false), (0, 1.0, false));
        assert_eq!(headshot_rule(true, 1, 1, false), (0, 1.0, true));
        for kind in [2, 3] {
            assert_eq!(headshot_rule(true, kind, 0, true), (0, 1.5, true));
            assert_eq!(headshot_rule(true, kind, 1, false), (1, 1.0, false));
        }
        for kind in [0, 1, 4] {
            assert_eq!(headshot_rule(true, kind, 2, false), (1, 1.0, false));
        }
        assert_eq!(headshot_rule(true, 0, 1, true), (1, 1.0, false));
        // Each damage context supplies a fresh multiplier, never the preceding attack's output.
        assert_eq!(headshot_rule(true, 2, 0, true).1, 1.5);
        assert_eq!(headshot_rule(true, 1, 1, false).1, 1.0);
    }
}
