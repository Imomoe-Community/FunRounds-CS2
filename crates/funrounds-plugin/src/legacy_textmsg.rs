// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use plugify::Str;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicI16, Ordering};

const LEGACY_AWARD_KEYS: &str = include_str!("../assets/legacy_textmsg_award_keys.txt");
const TEXTMSG: &str = "TextMsg";
const PARAM: &str = "param";

static HOOKED: AtomicBool = AtomicBool::new(false);
static FILTER_ENABLED: AtomicBool = AtomicBool::new(false);
static MESSAGE_ID: AtomicI16 = AtomicI16::new(0);

pub(crate) fn set_enabled(enabled: bool) {
    FILTER_ENABLED.store(enabled, Ordering::Release);
}

pub(super) fn hooked() -> bool {
    HOOKED.load(Ordering::Acquire)
}

pub(super) fn install() -> Result<(), &'static str> {
    if HOOKED.load(Ordering::Acquire) {
        return Err("previous TextMsg hook remains");
    }
    if !imports_available() {
        return Err("SDK user-message APIs unavailable; legacy award filtering is disabled");
    }

    let message_id = crate::s2sdk::UserMessageFindMessageIdByName(&Str::from_str(TEXTMSG));
    if message_id <= 0 {
        return Err("TextMsg message ID unavailable; legacy award filtering is disabled");
    }
    if !crate::s2sdk::HookUserMessage(message_id, on_text_msg, crate::s2sdk::HookMode::Pre) {
        return Err("TextMsg pre-hook registration failed; legacy award filtering is disabled");
    }

    MESSAGE_ID.store(message_id, Ordering::Release);
    HOOKED.store(true, Ordering::Release);
    Ok(())
}

pub(super) fn uninstall() -> bool {
    set_enabled(false);
    if !HOOKED.load(Ordering::Acquire) {
        return true;
    }
    if !imports_available() {
        return false;
    }

    let message_id = MESSAGE_ID.load(Ordering::Acquire);
    if !crate::s2sdk::UnhookUserMessage(message_id, on_text_msg, crate::s2sdk::HookMode::Pre) {
        return false;
    }

    HOOKED.store(false, Ordering::Release);
    MESSAGE_ID.store(0, Ordering::Release);
    true
}

fn imports_available() -> bool {
    let imports = unsafe {
        [
            std::ptr::addr_of!(crate::s2sdk::__s2sdk_UserMessageFindMessageIdByName).read().is_some(),
            std::ptr::addr_of!(crate::s2sdk::__s2sdk_HookUserMessage).read().is_some(),
            std::ptr::addr_of!(crate::s2sdk::__s2sdk_UnhookUserMessage).read().is_some(),
            std::ptr::addr_of!(crate::s2sdk::__s2sdk_UserMessageGetRepeatedFieldCount)
                .read()
                .is_some(),
            std::ptr::addr_of!(crate::s2sdk::__s2sdk_PbGetRepeatedString).read().is_some(),
        ]
    };
    imports.iter().all(|available| *available)
}

fn is_legacy_award_key(key: &str) -> bool {
    LEGACY_AWARD_KEYS.lines().any(|candidate| {
        (candidate.starts_with("#Player_Cash_Award_") || candidate.starts_with("#Player_Point_Award_") || candidate.starts_with("#Team_Cash_Award_"))
            && candidate == key
    })
}

fn should_block(enabled: bool, key: &str) -> bool {
    enabled && is_legacy_award_key(key)
}

pub(super) unsafe extern "C" fn on_text_msg(user_message: usize) -> crate::s2sdk::ResultType {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if !crate::callbacks_active() || !FILTER_ENABLED.load(Ordering::Acquire) || user_message == 0 {
            return crate::s2sdk::ResultType::Continue;
        }

        let param = Str::from_str(PARAM);
        if crate::s2sdk::UserMessageGetRepeatedFieldCount(user_message, &param) <= 0 {
            return crate::s2sdk::ResultType::Continue;
        }

        let mut first_param = Str::default();
        if !crate::s2sdk::PbGetRepeatedString(user_message, &param, 0, &mut first_param) {
            return crate::s2sdk::ResultType::Continue;
        }

        if should_block(true, first_param.as_str()) {
            crate::s2sdk::ResultType::Stop
        } else {
            crate::s2sdk::ResultType::Continue
        }
    }));

    match result {
        Ok(result) => result,
        Err(_) => {
            crate::contain_failure("TextMsg callback panic contained");
            crate::s2sdk::ResultType::Continue
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitized_catalog_preserves_player_and_team_award_filters() {
        let keys = LEGACY_AWARD_KEYS.lines().filter(|line| !line.starts_with("# ")).collect::<Vec<_>>();
        assert_eq!(keys.len(), 39);
        for key in keys {
            assert!(should_block(true, key), "{key}");
            assert!(!should_block(false, key), "{key}");
        }
        for key in [
            "#Player_Cash_Award_Bomb_Planted",
            "#Player_Point_Award_Assist_Enemy",
            "#Team_Cash_Award_Win_Defuse_Bomb",
        ] {
            assert!(is_legacy_award_key(key));
        }
        assert!(!should_block(true, "#Synthetic_Unrelated_Notification"));
    }
}
