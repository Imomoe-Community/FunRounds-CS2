// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

pub type DamagePreCallback = unsafe extern "C" fn(i32, i32, i32, i32, i32, bool, f32, &mut f32, &mut bool) -> i32;
pub type DamagePreCallbackV2 = unsafe extern "C" fn(i32, i32, i32, i32, i32, bool, f32, u32, &mut f32, &mut bool) -> i32;
type Hook = unsafe extern "C" fn(DamagePreCallback) -> bool;
type HookV2 = unsafe extern "C" fn(DamagePreCallbackV2) -> bool;

pub struct Damage {
    pub victim: i32,
    pub attacker: i32,
    pub inflictor: i32,
    pub weapon_kind: i32,
    pub hit_group: i32,
    pub traced_head: bool,
    pub amount: f32,
    pub damage_type: Option<u32>,
}

impl Damage {
    pub fn is_head_hit(&self, player_attributed: bool) -> bool {
        player_attributed && matches!(headshot_rule(true, self.weapon_kind, self.hit_group, self.traced_head), (0, _, true))
    }
    pub fn headshot_rule(&self, player_attributed: bool, classic: bool) -> (i32, f32, bool) {
        if classic && self.damage_type.is_some_and(|kind| kind & 32 != 0) {
            return (0, 1.0, false);
        }
        headshot_rule(player_attributed, self.weapon_kind, self.hit_group, self.traced_head)
    }
}

#[unsafe(no_mangle)]
static mut __s2sdk_HookPlayerTakeDamagePre: Option<Hook> = None;
#[unsafe(no_mangle)]
static mut __s2sdk_UnhookPlayerTakeDamagePre: Option<Hook> = None;
#[unsafe(no_mangle)]
static mut __s2sdk_HookPlayerTakeDamagePreV2: Option<HookV2> = None;
#[unsafe(no_mangle)]
static mut __s2sdk_UnhookPlayerTakeDamagePreV2: Option<HookV2> = None;

pub enum Bridge {
    V1 { unhook: Hook, callback: DamagePreCallback },
    V2 { unhook: HookV2, callback: DamagePreCallbackV2 },
}

impl Bridge {
    pub fn subscribe(callback: DamagePreCallback, callback_v2: DamagePreCallbackV2) -> Option<Self> {
        if let (Some(hook), Some(unhook)) = unsafe { (__s2sdk_HookPlayerTakeDamagePreV2, __s2sdk_UnhookPlayerTakeDamagePreV2) } {
            // A refused subscription is an ownership failure, not a reason to try another ABI.
            return unsafe { hook(callback_v2) }.then_some(Self::V2 {
                unhook,
                callback: callback_v2,
            });
        }
        // Use the same nullable imports as the generated SDK bindings.
        let hook = unsafe { __s2sdk_HookPlayerTakeDamagePre }?;
        let unhook = unsafe { __s2sdk_UnhookPlayerTakeDamagePre }?;
        unsafe { hook(callback) }.then_some(Self::V1 { unhook, callback })
    }

    pub fn unsubscribe(&self) -> bool {
        match *self {
            Self::V1 { unhook, callback } => unsafe { unhook(callback) },
            Self::V2 { unhook, callback } => unsafe { unhook(callback) },
        }
    }

    pub fn has_damage_type(&self) -> bool {
        matches!(self, Self::V2 { .. })
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
            damage_type: Some(2),
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
    fn classic_headshot_modes_allow_native_self_attributed_fall_without_scaling() {
        let mut damage = Damage {
            victim: 42,
            attacker: 42,
            inflictor: 42,
            weapon_kind: 0,
            hit_group: -1,
            traced_head: false,
            amount: 450.0,
            damage_type: Some(32),
        };
        let (action, special, mark) = damage.headshot_rule(true, true);
        assert_eq!((action, special, mark), (0, 1.0, false));
        assert_eq!(
            funrounds_core::classic_damage_scale(Some((450, 100)), true, true, false).map(|common| common * special),
            Some(1.0)
        );
        assert_eq!(damage.headshot_rule(true, false).0, 1);
        damage.damage_type = Some(2);
        damage.weapon_kind = 1;
        damage.hit_group = 2;
        assert_eq!(damage.headshot_rule(true, true).0, 1);
        damage.hit_group = 1;
        assert_eq!(damage.headshot_rule(true, true), (0, 1.0, true));
    }

    static V1_CALLS: AtomicU32 = AtomicU32::new(0);
    static V2_CALLS: AtomicU32 = AtomicU32::new(0);

    unsafe extern "C" fn callback_v1(_: i32, _: i32, _: i32, _: i32, _: i32, _: bool, _: f32, _: &mut f32, _: &mut bool) -> i32 {
        0
    }
    unsafe extern "C" fn callback_v2(_: i32, _: i32, _: i32, _: i32, _: i32, _: bool, _: f32, _: u32, _: &mut f32, _: &mut bool) -> i32 {
        0
    }
    unsafe extern "C" fn hook_v1(_: DamagePreCallback) -> bool {
        V1_CALLS.fetch_add(1, Ordering::Relaxed);
        true
    }
    unsafe extern "C" fn hook_v2(_: DamagePreCallbackV2) -> bool {
        V2_CALLS.fetch_add(1, Ordering::Relaxed);
        true
    }
    unsafe extern "C" fn reject_v2(_: DamagePreCallbackV2) -> bool {
        V2_CALLS.fetch_add(1, Ordering::Relaxed);
        false
    }

    #[test]
    fn bridge_selects_one_abi_and_keeps_old_imports_usable_without_v2() {
        unsafe {
            __s2sdk_HookPlayerTakeDamagePre = Some(hook_v1);
            __s2sdk_UnhookPlayerTakeDamagePre = Some(hook_v1);
            __s2sdk_HookPlayerTakeDamagePreV2 = Some(hook_v2);
            __s2sdk_UnhookPlayerTakeDamagePreV2 = Some(hook_v2);
        }
        let bridge = Bridge::subscribe(callback_v1, callback_v2).unwrap();
        assert!(bridge.has_damage_type());
        assert_eq!(V1_CALLS.load(Ordering::Relaxed), 0);
        assert!(bridge.unsubscribe());
        assert_eq!(V2_CALLS.load(Ordering::Relaxed), 2);

        unsafe {
            __s2sdk_HookPlayerTakeDamagePreV2 = Some(reject_v2);
        }
        assert!(Bridge::subscribe(callback_v1, callback_v2).is_none());
        assert_eq!(V1_CALLS.load(Ordering::Relaxed), 0);

        unsafe {
            __s2sdk_HookPlayerTakeDamagePreV2 = None;
            __s2sdk_UnhookPlayerTakeDamagePreV2 = None;
        }
        let bridge = Bridge::subscribe(callback_v1, callback_v2).unwrap();
        assert!(!bridge.has_damage_type());
        assert!(bridge.unsubscribe());
        assert_eq!(V1_CALLS.load(Ordering::Relaxed), 2);
        unsafe {
            __s2sdk_HookPlayerTakeDamagePre = None;
            __s2sdk_UnhookPlayerTakeDamagePre = None;
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
