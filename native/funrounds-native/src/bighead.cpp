// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#include "bighead.hpp"

#include <algorithm>
#include <bit>
#include <cstdint>
#include <iterator>
#include <vector>

#include <polyhook/polyhook.hpp>
#include <plugin_export.h>
#include <plg/plugin.hpp>
#include <s2sdk/s2sdk.hpp>

#include "bighead_policy.hpp"
#include "engine.hpp"

namespace funrounds::native::detail {
namespace {

constexpr char kResetSignature[] = "FunRounds::BigHeadScaleReset";
constexpr char kResetHookName[] = "FunRounds::BigHeadScaleReset";
constexpr char kPawnClass[] = "CCSPlayerPawn";
constexpr char kScaleField[] = "m_flBigHeadScale";
constexpr char kRulesClass[] = "CCSGameRules";
constexpr char kMutatorsField[] = "m_nActiveMutators";
constexpr std::int32_t kInvalidHandle = -1;

struct BigHeadLayout {
    SchemaField scale{};
    SchemaField mutators{};
    bool ready{};
};

struct RulesIdentity {
    void* rules{};
};

struct BigHeadTarget {
    std::int32_t pawnHandle{kInvalidHandle};
    void* pawn{};
    float originalScale{};
    float lastScale{};
};

struct ResetHook {
    void* address{};
    polyhook::HookHandle handle{};
    bool attempted{};
};

BigHeadLayout g_layout;
std::vector<BigHeadTarget> g_targets;
ResetHook g_resetHook;
RulesIdentity g_rules;
bighead::MutatorBitLease g_mutatorLease{};
bool g_hasRules{};
bool g_hasMutatorLease{};
bool g_mapEndRegistered{};
bool g_faulted{};
bool g_faultLogged{};

polyhook::ResultType BigHeadResetPost(polyhook::HookHandle,
                                      polyhook::ParametersHandle,
                                      std::int32_t,
                                      polyhook::ReturnHandle,
                                      polyhook::CallbackType) noexcept;
void BigHeadMapEndCallback() noexcept;

bool RequiredImportsAvailable() noexcept {
    return __polyhook_HookDetour != nullptr && __polyhook_FindDetour != nullptr &&
           __polyhook_AddCallback != nullptr && __polyhook_IsCallbackRegistered != nullptr &&
           __polyhook_RemoveCallback != nullptr && __polyhook_AreCallbacksRegistered != nullptr &&
           __polyhook_UnhookDetour != nullptr && __polyhook_GetArgumentPointer != nullptr &&
           __s2sdk_GetGameRules != nullptr && __s2sdk_GetGameConfigSignature != nullptr &&
           __s2sdk_OnMapEnd_Register != nullptr && __s2sdk_OnMapEnd_Unregister != nullptr &&
           __s2sdk_EntHandleToEntPointer != nullptr && __s2sdk_EntPointerToEntHandle != nullptr &&
           __s2sdk_IsValidEntHandle != nullptr && __s2sdk_IsEntityPlayerPawn != nullptr &&
           __s2sdk_GetEntSchemaFloat2 != nullptr && __s2sdk_SetEntSchemaFloat2 != nullptr &&
           __s2sdk_GetEntSchema2 != nullptr && __s2sdk_SetEntSchema2 != nullptr;
}

void LatchBigHeadFailure(const char* message) noexcept {
    g_faulted = true;
    if (g_faultLogged) {
        return;
    }
    g_faultLogged = true;
    try {
        if (plg::Log != nullptr) {
            plg::Log(message, plg::Severity::Error, plg::source_location::current());
        }
    } catch (...) {
    }
}

bool ResolveBigHeadLayout() noexcept {
    if (g_layout.ready) {
        return true;
    }

    SchemaField scale{};
    SchemaField mutators{};
    if (!ResolveSchemaField(kPawnClass, kScaleField, sizeof(float), scale) || scale.offset < 0 ||
        !ResolveSchemaField(kRulesClass, kMutatorsField, sizeof(std::int32_t), mutators) ||
        mutators.offset < 0) {
        return false;
    }

    g_layout = {scale, mutators, true};
    return true;
}

void* CurrentRules() noexcept {
    try {
        return s2sdk::GetGameRules();
    } catch (...) {
        return nullptr;
    }
}

bool SameRules(const RulesIdentity& identity) noexcept {
    return identity.rules != nullptr && CurrentRules() == identity.rules;
}

std::uint32_t ReadMutators(void* rules) {
    const auto mask = s2sdk::GetEntSchema2(rules, kRulesClass, kMutatorsField, 0);
    return static_cast<std::uint32_t>(static_cast<std::uint64_t>(mask));
}

void WriteMutators(void* rules, std::uint32_t mask) {
    const auto signedMask = std::bit_cast<std::int32_t>(mask);
    s2sdk::SetEntSchema2(rules,
                         kRulesClass,
                         kMutatorsField,
                         static_cast<std::int64_t>(signedMask),
                         true,
                         0);
}

float ReadScale(void* pawn) {
    return static_cast<float>(s2sdk::GetEntSchemaFloat2(pawn, kPawnClass, kScaleField, 0));
}

void WriteScale(void* pawn, float scale) {
    s2sdk::SetEntSchemaFloat2(pawn, kPawnClass, kScaleField, static_cast<double>(scale), true, 0);
}

bool CurrentPawn(std::int32_t pawnHandle, void*& pawn) noexcept {
    try {
        if (pawnHandle == kInvalidHandle || !s2sdk::IsValidEntHandle(pawnHandle) || !IsPlayerPawn(pawnHandle)) {
            return false;
        }
        pawn = EntityFromHandle(pawnHandle);
        return pawn != nullptr && HandleFromEntity(pawn) == pawnHandle;
    } catch (...) {
        pawn = nullptr;
        return false;
    }
}

bool RegisterMapEndCallback() noexcept {
    if (g_mapEndRegistered) {
        return true;
    }
    try {
        s2sdk::OnMapEnd_Register(BigHeadMapEndCallback);
        g_mapEndRegistered = true;
        return true;
    } catch (...) {
        return false;
    }
}

bool UnregisterMapEndCallback() noexcept {
    if (!g_mapEndRegistered) {
        return true;
    }
    try {
        s2sdk::OnMapEnd_Unregister(BigHeadMapEndCallback);
        g_mapEndRegistered = false;
        return true;
    } catch (...) {
        LatchBigHeadFailure("Big Head map-end callback cleanup failed; native recovery remains pending");
        return false;
    }
}

void ForgetTargetsForMapChange() noexcept {
    // Entity handles can be recycled on the next map. Never resolve or restore
    // a Pawn snapshot after crossing this boundary.
    g_targets.clear();
    g_rules = {};
    g_mutatorLease = {};
    g_hasRules = false;
    g_hasMutatorLease = false;
}

bool RestoreMutatorBit() noexcept {
    if (!g_hasRules || !g_hasMutatorLease || !g_mutatorLease.owned) {
        g_rules = {};
        g_mutatorLease = {};
        g_hasRules = false;
        g_hasMutatorLease = false;
        return true;
    }

    if (!SameRules(g_rules)) {
        // The old rules object is gone or a new map already owns the global.
        ForgetTargetsForMapChange();
        return true;
    }

    try {
        const auto current = ReadMutators(g_rules.rules);
        const auto restored = bighead::RestoreOwnedMutatorBit(current, g_mutatorLease);
        if (restored != current) {
            WriteMutators(g_rules.rules, restored);
            if (ReadMutators(g_rules.rules) != restored) {
                    LatchBigHeadFailure("Big Head mutator bit restore was not confirmed");
                return false;
            }
        }
        g_rules = {};
        g_mutatorLease = {};
        g_hasRules = false;
        g_hasMutatorLease = false;
        return true;
    } catch (...) {
        LatchBigHeadFailure("Big Head mutator bit restore failed; native recovery remains pending");
        return false;
    }
}

bool EnsureRulesBit() noexcept {
    void* const rules = CurrentRules();
    if (rules == nullptr) {
        return false;
    }

    if (g_hasRules && !SameRules(g_rules)) {
        ForgetTargetsForMapChange();
    }

    try {
        auto current = ReadMutators(rules);
        if (!g_hasMutatorLease) {
            g_rules = {rules};
            g_hasRules = true;
            g_mutatorLease = bighead::BeginMutatorBitLease(current);
            g_hasMutatorLease = true;
        }

        if ((current & bighead::kBigHeadMutatorBit) == 0) {
            if (!g_mutatorLease.owned) {
                return false;
            }
            const auto enabled = bighead::EnableOwnedMutatorBit(current, g_mutatorLease);
            WriteMutators(rules, enabled);
            current = ReadMutators(rules);
            if ((current & bighead::kBigHeadMutatorBit) == 0) {
                return false;
            }
        }
        return true;
    } catch (...) {
        return false;
    }
}

bool DetachResetHook() noexcept {
    try {
        if (g_resetHook.address == nullptr) {
            return g_resetHook.handle == nullptr;
        }

        const auto current = polyhook::FindDetour(g_resetHook.address);
        if (current == nullptr) {
            g_resetHook = {};
            return true;
        }
        if (current != g_resetHook.handle) {
            LatchBigHeadFailure("Big Head reset detour identity changed; native recovery remains pending");
            return false;
        }

        if (polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Post, BigHeadResetPost)) {
            (void)polyhook::RemoveCallback(current, polyhook::CallbackType::Post, BigHeadResetPost);
            if (polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Post, BigHeadResetPost)) {
                LatchBigHeadFailure("Big Head reset callback removal failed; native recovery remains pending");
                return false;
            }
        }

        // PolyHook handles are process-wide. Leave another plugin's callback
        // and detour alone after removing this provider's subscription.
        if (polyhook::AreCallbacksRegistered(current)) {
            g_resetHook = {};
            return true;
        }

        (void)polyhook::UnhookDetour(g_resetHook.address);
        if (polyhook::FindDetour(g_resetHook.address) != nullptr) {
            LatchBigHeadFailure("Big Head reset detour removal failed; native recovery remains pending");
            return false;
        }
        g_resetHook = {};
        return true;
    } catch (...) {
        LatchBigHeadFailure("Big Head reset detour cleanup failed; native recovery remains pending");
        return false;
    }
}

bool EnsureResetHook() noexcept {
    try {
        void* const address = s2sdk::GetGameConfigSignature(GameConfig(), kResetSignature);
        if (address == nullptr) {
            return false;
        }
        if (g_resetHook.address != nullptr && g_resetHook.address != address && !DetachResetHook()) {
            return false;
        }

        auto handle = polyhook::FindDetour(address);
        if (g_resetHook.handle != nullptr && handle != nullptr && handle != g_resetHook.handle) {
            return false;
        }
        if (handle != nullptr && g_resetHook.handle == nullptr) {
            // A pre-existing detour may have been created with HookDetour2's
            // mid-hook ABI, which cannot provide the Pawn* parameter needed by
            // this post-reset callback. Join only a detour this typed provider
            // created and can identify.
            return false;
        }
        if (handle == nullptr) {
            g_resetHook.address = address;
            g_resetHook.attempted = true;
            const plg::vector<polyhook::DataType> arguments{polyhook::DataType::Pointer};
            handle = polyhook::HookDetour(address,
                                           polyhook::DataType::Void,
                                           arguments,
                                           -1,
                                           kResetHookName);
            g_resetHook.handle = handle;
            if (handle == nullptr) {
                handle = polyhook::FindDetour(address);
                if (handle != nullptr) {
                    g_resetHook.handle = handle;
                } else {
                    g_resetHook = {};
                    LatchBigHeadFailure("Big Head reset detour could not be installed");
                    return false;
                }
            }
        } else {
            g_resetHook.address = address;
            g_resetHook.handle = handle;
        }

        if (!polyhook::IsCallbackRegistered(handle, polyhook::CallbackType::Post, BigHeadResetPost)) {
            (void)polyhook::AddCallback(handle, polyhook::CallbackType::Post, BigHeadResetPost);
        }
        if (!polyhook::IsCallbackRegistered(handle, polyhook::CallbackType::Post, BigHeadResetPost)) {
            (void)DetachResetHook();
            LatchBigHeadFailure("Big Head reset callback could not be installed");
            return false;
        }
        return true;
    } catch (...) {
        (void)DetachResetHook();
        LatchBigHeadFailure("Big Head reset hook initialization failed");
        return false;
    }
}

bool CleanupWhenEmpty() noexcept {
    if (!g_targets.empty()) {
        return true;
    }
    const bool bitClean = RestoreMutatorBit();
    const bool hookClean = DetachResetHook();
    // Keep the map-end recovery path until the GameRules lease is restored or
    // the map boundary invalidates that lease. Unregistering here would leave
    // a retained rules pointer vulnerable to address reuse on the next map.
    const bool callbackClean = bitClean && UnregisterMapEndCallback();
    const bool clean = bitClean && hookClean && callbackClean;
    if (clean) {
        g_faulted = false;
        g_faultLogged = false;
    }
    return clean;
}

bool BigHeadSupportAvailable() noexcept {
    if (g_faulted || !RequiredImportsAvailable() || !EnsureEngine() || !ResolveBigHeadLayout()) {
        return false;
    }
    try {
        void* const address = s2sdk::GetGameConfigSignature(GameConfig(), kResetSignature);
        if (address == nullptr) {
            return false;
        }
        const auto current = polyhook::FindDetour(address);
        if (current != nullptr && (g_resetHook.address != address || g_resetHook.handle != current)) {
            return false;
        }
        if (g_hasRules && g_hasMutatorLease && SameRules(g_rules) && !g_mutatorLease.owned &&
            (ReadMutators(g_rules.rules) & bighead::kBigHeadMutatorBit) == 0) {
            LatchBigHeadFailure("Big Head mutator bit owned by another system was cleared");
            return false;
        }
        return true;
    } catch (...) {
        if (!g_targets.empty() || g_hasMutatorLease) {
            LatchBigHeadFailure("Big Head support check failed while native state was active");
        }
        return false;
    }
}

bool ClearTarget(std::int32_t pawnHandle) noexcept {
    const auto found = std::find_if(g_targets.begin(), g_targets.end(), [pawnHandle](const auto& target) {
        return target.pawnHandle == pawnHandle;
    });
    if (found == g_targets.end()) {
        return CleanupWhenEmpty();
    }

    try {
        void* pawn = nullptr;
        if (CurrentPawn(found->pawnHandle, pawn) && pawn == found->pawn) {
            const float current = ReadScale(pawn);
            if (bighead::StillOwnsScale(current, found->lastScale) &&
                !bighead::StillOwnsScale(current, found->originalScale)) {
                WriteScale(pawn, found->originalScale);
                if (!bighead::StillOwnsScale(ReadScale(pawn), found->originalScale)) {
                    LatchBigHeadFailure("Big Head scale restore was not confirmed; native recovery remains pending");
                    return false;
                }
            }
        }
        g_targets.erase(found);
        return CleanupWhenEmpty();
    } catch (...) {
        LatchBigHeadFailure("Big Head scale restore failed; native recovery remains pending");
        return false;
    }
}

bool WriteTargetScale(BigHeadTarget& target, float scale) noexcept {
    const float previousLast = target.lastScale;
    try {
        target.lastScale = scale;
        WriteScale(target.pawn, scale);
        const float observed = ReadScale(target.pawn);
        if (bighead::StillOwnsScale(observed, scale)) {
            target.lastScale = observed;
            return true;
        }
        if (bighead::StillOwnsScale(observed, previousLast)) {
            target.lastScale = previousLast;
        }
        return false;
    } catch (...) {
        try {
            const float observed = ReadScale(target.pawn);
            if (bighead::StillOwnsScale(observed, previousLast)) {
                target.lastScale = previousLast;
            } else if (bighead::StillOwnsScale(observed, scale)) {
                target.lastScale = scale;
            }
        } catch (...) {
        }
        return false;
    }
}

bool SetTarget(std::int32_t pawnHandle, float scale) noexcept {
    if (g_faulted || !bighead::IsValidScale(scale) || !RequiredImportsAvailable() || !EnsureEngine() ||
        !ResolveBigHeadLayout()) {
        return false;
    }

    void* pawn = nullptr;
    if (!CurrentPawn(pawnHandle, pawn)) {
        return false;
    }
    if (!RegisterMapEndCallback()) {
        LatchBigHeadFailure("Big Head map-end recovery callback could not be registered");
        (void)CleanupWhenEmpty();
        return false;
    }
    if (!EnsureResetHook()) {
        LatchBigHeadFailure("Big Head reset hook is unavailable");
        if (g_targets.empty()) {
            (void)CleanupWhenEmpty();
        }
        return false;
    }

    if (!EnsureRulesBit()) {
        LatchBigHeadFailure("Big Head mutator bit is unavailable or no longer owned");
        if (g_targets.empty()) {
            (void)CleanupWhenEmpty();
        }
        return false;
    }

    try {
        auto found = std::find_if(g_targets.begin(), g_targets.end(), [pawnHandle](const auto& target) {
            return target.pawnHandle == pawnHandle;
        });
        const float current = ReadScale(pawn);
        bool inserted = false;
        if (found == g_targets.end()) {
            g_targets.push_back({pawnHandle, pawn, current, current});
            found = std::prev(g_targets.end());
            inserted = true;
        } else if (found->pawn != pawn) {
            // A serial-bearing handle must resolve to the same object. Treat a
            // mismatch as a new identity only after relinquishing the old lease.
            g_targets.erase(found);
            g_targets.push_back({pawnHandle, pawn, current, current});
            found = std::prev(g_targets.end());
            inserted = true;
        } else if (!bighead::StillOwnsScale(current, found->lastScale)) {
            // A later explicit set rebases restoration onto the external value
            // that currently owns the field instead of restoring a stale lease.
            found->originalScale = current;
            found->lastScale = current;
        }

        if (WriteTargetScale(*found, scale)) {
            return true;
        }
        LatchBigHeadFailure("Big Head scale write was not confirmed");
        if (inserted) {
            void* currentPawn = nullptr;
            if (!CurrentPawn(pawnHandle, currentPawn) || currentPawn != pawn ||
                !bighead::StillOwnsScale(ReadScale(pawn), found->lastScale)) {
                g_targets.erase(found);
            }
        }
        if (g_targets.empty()) {
            (void)CleanupWhenEmpty();
        }
        return false;
    } catch (...) {
        LatchBigHeadFailure("Big Head target registration failed; native recovery remains pending");
        if (g_targets.empty()) {
            (void)CleanupWhenEmpty();
        }
        return false;
    }
}

void BigHeadMapEndCallback() noexcept {
    // Map end is the safe invalidation boundary for the old rules and Pawn
    // identities. Do not write through GameRules here: an engine may already
    // have reused that address for the next map.
    ForgetTargetsForMapChange();
    if (!DetachResetHook()) {
        LatchBigHeadFailure("Big Head map-end reset hook removal failed");
        return;
    }
    // The old map's leases are now discarded and its reset hook is detached;
    // a prior map-scoped failure no longer blocks the next map.
    g_faulted = false;
    g_faultLogged = false;
}

polyhook::ResultType BigHeadResetPost(polyhook::HookHandle,
                                      polyhook::ParametersHandle parameters,
                                      std::int32_t,
                                      polyhook::ReturnHandle,
                                      polyhook::CallbackType type) noexcept {
    if (g_faulted || type != polyhook::CallbackType::Post || parameters == nullptr || g_targets.empty()) {
        return polyhook::ResultType::Ignored;
    }

    try {
        void* const pawn = polyhook::GetArgumentPointer(parameters, 0);
        if (pawn == nullptr) {
            return polyhook::ResultType::Ignored;
        }
        const auto pawnHandle = HandleFromEntity(pawn);
        void* currentPawn = nullptr;
        if (!CurrentPawn(pawnHandle, currentPawn) || currentPawn != pawn) {
            return polyhook::ResultType::Ignored;
        }

        const auto found = std::find_if(g_targets.begin(), g_targets.end(), [pawnHandle](const auto& target) {
            return target.pawnHandle == pawnHandle;
        });
        if (found == g_targets.end() || found->pawn != pawn) {
            return polyhook::ResultType::Ignored;
        }
        if (!SameRules(g_rules)) {
            ForgetTargetsForMapChange();
            return polyhook::ResultType::Ignored;
        }

        if (!EnsureRulesBit()) {
            LatchBigHeadFailure("Big Head post-reset mutator bit is unavailable or no longer owned");
            return polyhook::ResultType::Ignored;
        }
        if (!WriteTargetScale(*found, found->lastScale)) {
            LatchBigHeadFailure("Big Head post-reset scale write was not confirmed");
        }
    } catch (...) {
        LatchBigHeadFailure("Big Head post-reset recovery failed; native recovery remains pending");
    }
    return polyhook::ResultType::Ignored;
}

} // namespace

bool ShutdownBigHead() noexcept {
    try {
        while (!g_targets.empty()) {
            const auto pawnHandle = g_targets.front().pawnHandle;
            const auto before = g_targets.size();
            if (!ClearTarget(pawnHandle) || g_targets.size() == before) {
                return false;
            }
        }

        const bool bitClean = RestoreMutatorBit();
        const bool hookClean = DetachResetHook();
        const bool callbackClean = bitClean && UnregisterMapEndCallback();
        if (!bitClean || !hookClean || !callbackClean) {
            return false;
        }
        g_layout = {};
        return true;
    } catch (...) {
        return false;
    }
}

} // namespace funrounds::native::detail

extern "C" PLUGIN_API bool HasBigHeadScaleSupport() {
    return funrounds::native::detail::BigHeadSupportAvailable();
}

extern "C" PLUGIN_API bool SetBigHeadScaleTarget(std::int32_t pawnHandle, float scale) {
    return funrounds::native::detail::SetTarget(pawnHandle, scale);
}

extern "C" PLUGIN_API bool ClearBigHeadScaleTarget(std::int32_t pawnHandle) {
    return funrounds::native::detail::ClearTarget(pawnHandle);
}
