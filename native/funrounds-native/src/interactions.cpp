// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#include "interactions.hpp"

#include <array>
#include <bit>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <string_view>

#include <polyhook/polyhook.hpp>
#include <plugin_export.h>
#include <s2sdk/s2sdk.hpp>

#include "button_state.hpp"
#include "chicken_follow_policy.hpp"
#include "engine.hpp"

namespace funrounds::native::detail {
namespace {

constexpr std::string_view kUseTargetSignature = "OuterUseTargetSelector";
constexpr std::string_view kUseTargetOffset = "UseServicesLastTargetHandle";
constexpr std::string_view kHealthshotCanUseSignature = "HealthshotCanUse";
constexpr std::string_view kHealthshotEffectSignature = "HealthshotSuccessEffect";
constexpr std::string_view kChickenFollowUseSignature = "CChickenChickenUse";
constexpr std::string_view kUseTargetHookName = "FunRounds::PlayerUseTargetSelector";
constexpr std::string_view kHealthshotCanUseHookName = "FunRounds::HealthshotCanUse";
constexpr std::string_view kHealthshotEffectHookName = "FunRounds::HealthshotSuccessEffect";
constexpr std::string_view kChickenFollowUseHookName = "FunRounds::ChickenFollowUse";
constexpr std::int32_t kInvalidHandle = -1;
constexpr std::uint64_t kInUseButton = 0x20;
constexpr std::uint32_t kInvalidEntityBits = std::numeric_limits<std::uint32_t>::max();
constexpr std::array<std::string_view, 3> kPawnClasses{
    "CBasePlayerPawn",
    "CCSPlayerPawnBase",
    "CCSPlayerPawn",
};

struct UseTargetState {
    void* address{};
    polyhook::HookHandle hook{};
    UseTargetCallback owner{};
    UseTargetCallback callback{};
    std::array<std::size_t, kPawnClasses.size()> schemaClassIndices{};
    std::size_t schemaCount{};
    std::int32_t committedTargetOffset{-1};
    bool callbackInstalled{};
};

struct HealthshotState {
    void* canUseAddress{};
    polyhook::HookHandle canUseHook{};
    void* effectAddress{};
    polyhook::HookHandle effectHook{};
    HealthshotCanUseCallback canUseOwner{};
    HealthshotCanUseCallback canUseCallback{};
    HealthshotEffectCallback effectOwner{};
    HealthshotEffectCallback effectCallback{};
    bool canUseCallbackInstalled{};
    bool effectCallbackInstalled{};
};

struct ChickenFollowUseState {
    void* address{};
    polyhook::HookHandle hook{};
    bool callbackInstalled{};
    bool active{};
};

UseTargetState g_useTarget;
HealthshotState g_healthshot;
ChickenFollowUseState g_chickenFollowUse;

bool UseTargetEmpty() noexcept {
    return !g_useTarget.address && !g_useTarget.hook && !g_useTarget.owner && !g_useTarget.callback &&
           !g_useTarget.callbackInstalled;
}

bool HealthshotEmpty() noexcept {
    return !g_healthshot.canUseAddress && !g_healthshot.canUseHook && !g_healthshot.effectAddress &&
           !g_healthshot.effectHook && !g_healthshot.canUseOwner && !g_healthshot.canUseCallback &&
           !g_healthshot.effectOwner && !g_healthshot.effectCallback && !g_healthshot.canUseCallbackInstalled &&
           !g_healthshot.effectCallbackInstalled;
}

bool ChickenFollowUseEmpty() noexcept {
    return !g_chickenFollowUse.address && !g_chickenFollowUse.hook &&
           !g_chickenFollowUse.callbackInstalled && !g_chickenFollowUse.active;
}

polyhook::ResultType UseTargetPost(polyhook::HookHandle,
                                   polyhook::ParametersHandle,
                                   std::int32_t,
                                   polyhook::ReturnHandle,
                                   polyhook::CallbackType) noexcept;
polyhook::ResultType HealthshotCanUsePre(polyhook::HookHandle,
                                         polyhook::ParametersHandle,
                                         std::int32_t,
                                         polyhook::ReturnHandle,
                                         polyhook::CallbackType) noexcept;
polyhook::ResultType HealthshotEffectPost(polyhook::HookHandle,
                                          polyhook::ParametersHandle,
                                          std::int32_t,
                                          polyhook::ReturnHandle,
                                          polyhook::CallbackType) noexcept;
polyhook::ResultType ChickenFollowUsePre(polyhook::HookHandle,
                                         polyhook::ParametersHandle,
                                         std::int32_t,
                                         polyhook::ReturnHandle,
                                         polyhook::CallbackType) noexcept;

bool HookImportsAvailable() noexcept {
    return __polyhook_HookDetour && __polyhook_FindDetour && __polyhook_AddCallback &&
           __polyhook_IsCallbackRegistered && __polyhook_RemoveCallback && __polyhook_UnhookDetour &&
           __polyhook_GetArgumentPointer && __polyhook_GetArgumentFloat && __polyhook_GetArgumentInt32 &&
           __polyhook_GetReturnPointer && __polyhook_SetReturnBool && __s2sdk_GetGameConfigSignature &&
           __s2sdk_GetGameConfigOffset && __s2sdk_GetSchemaOffset && __s2sdk_GetEntSchema2 && __s2sdk_GetEntSchemaEnt2 &&
           __s2sdk_GetMaxClients && __s2sdk_IsClientInGame && __s2sdk_GetClientPawn &&
           __s2sdk_PlayerSlotToEntHandle &&
           __s2sdk_GetClientButtons && __s2sdk_GetClientWeapons && __s2sdk_GetEntityClassname &&
           __s2sdk_EntHandleToEntPointer && __s2sdk_EntPointerToEntHandle && __s2sdk_IsValidEntHandle &&
           __s2sdk_IsEntityPlayerPawn;
}

bool ChickenFollowImportsAvailable() noexcept {
    return HookImportsAvailable();
}

bool ResolveUseTargetMetadata() noexcept {
    if (!EnsureEngine() || !__s2sdk_GetGameConfigOffset || !__s2sdk_GetSchemaOffset || !__s2sdk_GetEntSchema2) {
        return false;
    }
    if (g_useTarget.committedTargetOffset < 0) {
        g_useTarget.committedTargetOffset = s2sdk::GetGameConfigOffset(GameConfig(), plg::string(kUseTargetOffset));
    }
    if (g_useTarget.committedTargetOffset < 0) {
        return false;
    }
    if (g_useTarget.schemaCount == 0) {
        for (std::size_t index = 0; index < kPawnClasses.size(); ++index) {
            const auto className = kPawnClasses[index];
            const auto offset = s2sdk::GetSchemaOffset(plg::string(className), plg::string("m_pUseServices"));
            if (offset >= 0) {
                g_useTarget.schemaClassIndices[g_useTarget.schemaCount++] = index;
            }
        }
    }
    return g_useTarget.schemaCount != 0;
}

void* Signature(std::string_view name) noexcept {
    if (!EnsureEngine() || !__s2sdk_GetGameConfigSignature) {
        return nullptr;
    }
    return s2sdk::GetGameConfigSignature(GameConfig(), plg::string(name));
}

bool SlotOwnsPawn(std::int32_t slot, void* pawn) noexcept {
    if (!pawn || !s2sdk::IsClientInGame(slot) || s2sdk::GetClientPawn(slot) != pawn) {
        return false;
    }
    const auto controller = s2sdk::PlayerSlotToEntHandle(slot);
    if (!s2sdk::IsValidEntHandle(controller) ||
        s2sdk::GetSchemaOffset(plg::string("CBasePlayerPawn"), plg::string("m_hController")) < 0) {
        return false;
    }
    // A stale BOT slot may retain m_hPawn after takeover. The pawn's reverse link is authoritative.
    return s2sdk::GetEntSchemaEnt2(pawn, plg::string("CBasePlayerPawn"), plg::string("m_hController"), 0) == controller;
}

bool ResolveUseServiceOwner(void* services, std::int32_t& pawnHandle, std::int32_t& slot) noexcept {
    if (!services || !ResolveUseTargetMetadata()) {
        return false;
    }
    std::size_t matches = 0;
    const auto maximum = s2sdk::GetMaxClients();
    if (maximum <= 0) {
        return false;
    }
    for (std::int32_t candidateSlot = 0; candidateSlot < maximum; ++candidateSlot) {
        if (!s2sdk::IsClientInGame(candidateSlot)) {
            continue;
        }
        void* const pawn = s2sdk::GetClientPawn(candidateSlot);
        const auto candidateHandle = HandleFromEntity(pawn);
        if (!pawn || candidateHandle == kInvalidHandle || !IsPlayerPawn(candidateHandle) ||
            EntityFromHandle(candidateHandle) != pawn || !SlotOwnsPawn(candidateSlot, pawn)) {
            continue;
        }
        bool serviceMatches = false;
        for (std::size_t index = 0; index < g_useTarget.schemaCount; ++index) {
            const auto className = kPawnClasses[g_useTarget.schemaClassIndices[index]];
            const auto serviceAddress = s2sdk::GetEntSchema2(pawn, plg::string(className),
                                                              plg::string("m_pUseServices"), 0);
            if (serviceAddress == static_cast<std::int64_t>(reinterpret_cast<std::uintptr_t>(services))) {
                serviceMatches = true;
                break;
            }
        }
        if (serviceMatches) {
            ++matches;
            pawnHandle = candidateHandle;
            slot = candidateSlot;
        }
    }
    return matches == 1 && SlotOwnsPawn(slot, EntityFromHandle(pawnHandle));
}

bool UseButtonDown(std::int32_t slot) noexcept {
    const auto currentButtons = s2sdk::GetClientButtons(
        slot, static_cast<std::int32_t>(ButtonStateGroup::CurrentDown));
    return IsButtonCurrentlyDown(currentButtons, kInUseButton);
}

std::int32_t TargetHandle(void* target) noexcept {
    if (!target) {
        return kInvalidHandle;
    }
    const auto handle = HandleFromEntity(target);
    if (handle == kInvalidHandle || !s2sdk::IsValidEntHandle(handle) || EntityFromHandle(handle) != target) {
        return kInvalidHandle;
    }
    return handle;
}

bool ResolveHealthshotOwner(void* weapon, void* pawn, std::int32_t& pawnHandle,
                            std::int32_t& weaponHandle) noexcept {
    if (!weapon || !pawn) {
        return false;
    }
    const auto resolvedPawn = HandleFromEntity(pawn);
    if (resolvedPawn == kInvalidHandle || !IsPlayerPawn(resolvedPawn) || EntityFromHandle(resolvedPawn) != pawn) {
        return false;
    }
    std::size_t ownerMatches = 0;
    std::int32_t matchedSlot = -1;
    std::int32_t matchedWeapon = kInvalidHandle;
    const auto maximum = s2sdk::GetMaxClients();
    if (maximum <= 0) {
        return false;
    }
    for (std::int32_t candidateSlot = 0; candidateSlot < maximum; ++candidateSlot) {
        if (!SlotOwnsPawn(candidateSlot, pawn)) {
            continue;
        }
        std::size_t weaponMatches = 0;
        for (const auto handle : s2sdk::GetClientWeapons(candidateSlot)) {
            if (handle == kInvalidHandle || !s2sdk::IsValidEntHandle(handle) ||
                s2sdk::GetEntityClassname(handle) != "weapon_healthshot") {
                continue;
            }
            if (EntityFromHandle(handle) == weapon) {
                ++weaponMatches;
                matchedWeapon = handle;
            }
        }
        if (weaponMatches == 1) {
            ++ownerMatches;
            matchedSlot = candidateSlot;
        }
    }
    if (ownerMatches != 1 || matchedSlot < 0 || matchedWeapon == kInvalidHandle ||
        !SlotOwnsPawn(matchedSlot, pawn) || HandleFromEntity(pawn) != resolvedPawn) {
        return false;
    }
    pawnHandle = resolvedPawn;
    weaponHandle = matchedWeapon;
    return true;
}

bool RemoveUseTargetHook() noexcept {
    g_useTarget.callback = nullptr;
    if (!g_useTarget.address) {
        const bool clean = !g_useTarget.hook;
        g_useTarget = {};
        return clean;
    }
    try {
        const auto current = polyhook::FindDetour(g_useTarget.address);
        if (!current) {
            g_useTarget = {};
            return true;
        }
        if (!g_useTarget.hook || current != g_useTarget.hook) {
            return false;
        }
        if (g_useTarget.callbackInstalled ||
            polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Post, UseTargetPost)) {
            (void)polyhook::RemoveCallback(current, polyhook::CallbackType::Post, UseTargetPost);
            if (polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Post, UseTargetPost)) {
                return false;
            }
        }
        if (!polyhook::UnhookDetour(g_useTarget.address) && polyhook::FindDetour(g_useTarget.address)) {
            return false;
        }
        if (polyhook::FindDetour(g_useTarget.address)) {
            return false;
        }
        g_useTarget = {};
        return true;
    } catch (...) {
        return false;
    }
}

bool RemoveHealthshotHooks() noexcept {
    g_healthshot.canUseCallback = nullptr;
    g_healthshot.effectCallback = nullptr;
    auto remove = [](polyhook::HookHandle& hook, void*& address, polyhook::CallbackType mode,
                     auto callback, bool& installed) noexcept {
        if (!hook) {
            installed = false;
            address = nullptr;
            return true;
        }
        try {
            const auto current = address ? polyhook::FindDetour(address) : nullptr;
            if (!current) {
                hook = nullptr;
                address = nullptr;
                installed = false;
                return true;
            }
            if (current != hook) {
                return false;
            }
            if (installed || polyhook::IsCallbackRegistered(current, mode, callback)) {
                (void)polyhook::RemoveCallback(current, mode, callback);
                if (polyhook::IsCallbackRegistered(current, mode, callback)) {
                    return false;
                }
            }
            installed = false;
            if (!polyhook::UnhookDetour(address) && polyhook::FindDetour(address)) {
                return false;
            }
            if (polyhook::FindDetour(address)) {
                return false;
            }
            hook = nullptr;
            address = nullptr;
            return true;
        } catch (...) {
            return false;
        }
    };

    bool clean = remove(g_healthshot.canUseHook, g_healthshot.canUseAddress,
                        polyhook::CallbackType::Pre, HealthshotCanUsePre,
                        g_healthshot.canUseCallbackInstalled);
    clean = remove(g_healthshot.effectHook, g_healthshot.effectAddress,
                   polyhook::CallbackType::Post, HealthshotEffectPost,
                   g_healthshot.effectCallbackInstalled) && clean;
    if (clean) {
        g_healthshot = {};
    }
    return clean;
}

bool CurrentChickenUseEntity(void* entity) {
    if (!entity || !__s2sdk_EntPointerToEntHandle || !__s2sdk_IsValidEntHandle ||
        !__s2sdk_EntHandleToEntPointer || !__s2sdk_GetEntityClassname) {
        return false;
    }
    const auto handle = s2sdk::EntPointerToEntHandle(entity);
    return handle != kInvalidHandle && s2sdk::IsValidEntHandle(handle) &&
           s2sdk::EntHandleToEntPointer(handle) == entity && s2sdk::GetEntityClassname(handle) == "chicken";
}

bool RemoveChickenFollowUseHook() noexcept {
    g_chickenFollowUse.active = false;
    if (!g_chickenFollowUse.address) {
        const bool clean = !g_chickenFollowUse.hook && !g_chickenFollowUse.callbackInstalled;
        if (clean) {
            g_chickenFollowUse = {};
        }
        return clean;
    }
    try {
        const auto current = polyhook::FindDetour(g_chickenFollowUse.address);
        if (!current) {
            g_chickenFollowUse = {};
            return true;
        }
        if (!g_chickenFollowUse.hook || current != g_chickenFollowUse.hook) {
            return false;
        }
        if (g_chickenFollowUse.callbackInstalled ||
            polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, ChickenFollowUsePre)) {
            (void)polyhook::RemoveCallback(current, polyhook::CallbackType::Pre, ChickenFollowUsePre);
            if (polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, ChickenFollowUsePre)) {
                return false;
            }
            g_chickenFollowUse.callbackInstalled = false;
        }
        if (!polyhook::UnhookDetour(g_chickenFollowUse.address) &&
            polyhook::FindDetour(g_chickenFollowUse.address)) {
            return false;
        }
        if (polyhook::FindDetour(g_chickenFollowUse.address)) {
            return false;
        }
        g_chickenFollowUse = {};
        return true;
    } catch (...) {
        return false;
    }
}

bool InstallChickenFollowUseHook() {
    if (!ChickenFollowImportsAvailable()) {
        return false;
    }
    const auto address = Signature(kChickenFollowUseSignature);
    if (!address || (g_chickenFollowUse.address && g_chickenFollowUse.address != address)) {
        return false;
    }
    if (g_chickenFollowUse.hook) {
        if (polyhook::FindDetour(address) != g_chickenFollowUse.hook ||
            !polyhook::IsCallbackRegistered(g_chickenFollowUse.hook, polyhook::CallbackType::Pre,
                                             ChickenFollowUsePre)) {
            return false;
        }
        g_chickenFollowUse.active = true;
        return true;
    }
    if (polyhook::FindDetour(address)) {
        return false;
    }

    // Preserve the full CChickenChickenUse ABI: (this, const EntityUseInput_t*) -> void.
    const std::array arguments{polyhook::DataType::Pointer, polyhook::DataType::Pointer};
    g_chickenFollowUse.address = address;
    g_chickenFollowUse.hook = polyhook::HookDetour(
        address, polyhook::DataType::Void, plg::vector<polyhook::DataType>(arguments.begin(), arguments.end()), -1,
        plg::string(kChickenFollowUseHookName));
    if (!g_chickenFollowUse.hook) {
        (void)RemoveChickenFollowUseHook();
        return false;
    }
    try {
        (void)polyhook::AddCallback(g_chickenFollowUse.hook, polyhook::CallbackType::Pre, ChickenFollowUsePre);
        g_chickenFollowUse.callbackInstalled = polyhook::IsCallbackRegistered(
            g_chickenFollowUse.hook, polyhook::CallbackType::Pre, ChickenFollowUsePre);
        if (!g_chickenFollowUse.callbackInstalled) {
            (void)RemoveChickenFollowUseHook();
            return false;
        }
        g_chickenFollowUse.active = true;
        return true;
    } catch (...) {
        (void)RemoveChickenFollowUseHook();
        return false;
    }
}

bool InstallUseTargetHook(UseTargetCallback callback) {
    if (!callback || !HookImportsAvailable() || !ResolveUseTargetMetadata()) {
        return false;
    }
    const auto address = Signature(kUseTargetSignature);
    if (!address || (g_useTarget.address && g_useTarget.address != address)) {
        return false;
    }
    if (g_useTarget.hook) {
        if (g_useTarget.owner != callback || polyhook::FindDetour(address) != g_useTarget.hook ||
            !polyhook::IsCallbackRegistered(g_useTarget.hook, polyhook::CallbackType::Post, UseTargetPost)) {
            return false;
        }
        g_useTarget.callback = callback;
        return true;
    }
    if (polyhook::FindDetour(address)) {
        return false;
    }
    const std::array arguments{polyhook::DataType::Pointer, polyhook::DataType::Float};
    g_useTarget.address = address;
    g_useTarget.hook = polyhook::HookDetour(address, polyhook::DataType::Pointer,
        plg::vector<polyhook::DataType>(arguments.begin(), arguments.end()), -1,
        plg::string(kUseTargetHookName));
    if (!g_useTarget.hook) {
        (void)RemoveUseTargetHook();
        return false;
    }
    try {
        (void)polyhook::AddCallback(g_useTarget.hook, polyhook::CallbackType::Post, UseTargetPost);
        g_useTarget.callbackInstalled = polyhook::IsCallbackRegistered(
            g_useTarget.hook, polyhook::CallbackType::Post, UseTargetPost);
        if (!g_useTarget.callbackInstalled) {
            (void)RemoveUseTargetHook();
            return false;
        }
        g_useTarget.owner = callback;
        g_useTarget.callback = callback;
        return true;
    } catch (...) {
        (void)RemoveUseTargetHook();
        return false;
    }
}

bool InstallHealthshotHooks(HealthshotCanUseCallback canUse, HealthshotEffectCallback effect) {
    if (!canUse || !effect || !HookImportsAvailable()) {
        return false;
    }
    const auto canUseAddress = Signature(kHealthshotCanUseSignature);
    const auto effectAddress = Signature(kHealthshotEffectSignature);
    if (!canUseAddress || !effectAddress ||
        (g_healthshot.canUseAddress && g_healthshot.canUseAddress != canUseAddress) ||
        (g_healthshot.effectAddress && g_healthshot.effectAddress != effectAddress)) {
        return false;
    }
    if (g_healthshot.canUseHook || g_healthshot.effectHook) {
        if (g_healthshot.canUseOwner != canUse || g_healthshot.effectOwner != effect ||
            polyhook::FindDetour(canUseAddress) != g_healthshot.canUseHook ||
            polyhook::FindDetour(effectAddress) != g_healthshot.effectHook ||
            !polyhook::IsCallbackRegistered(g_healthshot.canUseHook, polyhook::CallbackType::Pre, HealthshotCanUsePre) ||
            !polyhook::IsCallbackRegistered(g_healthshot.effectHook, polyhook::CallbackType::Post, HealthshotEffectPost)) {
            return false;
        }
        g_healthshot.canUseCallback = canUse;
        g_healthshot.effectCallback = effect;
        return true;
    }
    if (polyhook::FindDetour(canUseAddress) || polyhook::FindDetour(effectAddress)) {
        return false;
    }

    const std::array canUseArguments{polyhook::DataType::Pointer, polyhook::DataType::Pointer};
    g_healthshot.canUseAddress = canUseAddress;
    g_healthshot.canUseHook = polyhook::HookDetour(canUseAddress, polyhook::DataType::Bool,
        plg::vector<polyhook::DataType>(canUseArguments.begin(), canUseArguments.end()), -1,
        plg::string(kHealthshotCanUseHookName));
    if (!g_healthshot.canUseHook) {
        (void)RemoveHealthshotHooks();
        return false;
    }
    try {
        (void)polyhook::AddCallback(g_healthshot.canUseHook, polyhook::CallbackType::Pre, HealthshotCanUsePre);
        g_healthshot.canUseCallbackInstalled = polyhook::IsCallbackRegistered(
            g_healthshot.canUseHook, polyhook::CallbackType::Pre, HealthshotCanUsePre);
        if (!g_healthshot.canUseCallbackInstalled) {
            (void)RemoveHealthshotHooks();
            return false;
        }

#if defined(_WIN32)
        const std::array effectArguments{polyhook::DataType::Pointer, polyhook::DataType::Pointer,
                                         polyhook::DataType::Pointer, polyhook::DataType::Int32};
#elif defined(__linux__)
        const std::array effectArguments{polyhook::DataType::Pointer, polyhook::DataType::Pointer};
#else
#error "Healthshot interaction hook supports Windows x64 and Linux x64 only"
#endif
        g_healthshot.effectAddress = effectAddress;
        g_healthshot.effectHook = polyhook::HookDetour(effectAddress, polyhook::DataType::Void,
            plg::vector<polyhook::DataType>(effectArguments.begin(), effectArguments.end()), -1,
            plg::string(kHealthshotEffectHookName));
        if (!g_healthshot.effectHook) {
            (void)RemoveHealthshotHooks();
            return false;
        }
        (void)polyhook::AddCallback(g_healthshot.effectHook, polyhook::CallbackType::Post, HealthshotEffectPost);
        g_healthshot.effectCallbackInstalled = polyhook::IsCallbackRegistered(
            g_healthshot.effectHook, polyhook::CallbackType::Post, HealthshotEffectPost);
        if (!g_healthshot.effectCallbackInstalled) {
            (void)RemoveHealthshotHooks();
            return false;
        }
        g_healthshot.canUseOwner = canUse;
        g_healthshot.effectOwner = effect;
        g_healthshot.canUseCallback = canUse;
        g_healthshot.effectCallback = effect;
        return true;
    } catch (...) {
        (void)RemoveHealthshotHooks();
        return false;
    }
}

polyhook::ResultType UseTargetPost(polyhook::HookHandle,
                                   polyhook::ParametersHandle parameters,
                                   std::int32_t,
                                   polyhook::ReturnHandle result,
                                   polyhook::CallbackType type) noexcept {
    const auto callback = g_useTarget.callback;
    if (type != polyhook::CallbackType::Post || !callback || !parameters || !result) {
        return polyhook::ResultType::Ignored;
    }
    try {
        auto* const services = polyhook::GetArgument<void*>(parameters, 0);
        std::int32_t pawnHandle = kInvalidHandle;
        std::int32_t slot = -1;
        if (!ResolveUseServiceOwner(services, pawnHandle, slot)) {
            return polyhook::ResultType::Ignored;
        }
        const auto inUse = UseButtonDown(slot);
        if (!inUse) {
            callback(pawnHandle, kInvalidHandle, false);
            return polyhook::ResultType::Ignored;
        }
        auto* const selectedTarget = polyhook::GetReturn<void*>(result);
        const auto selectedHandle = TargetHandle(selectedTarget);
        std::uint32_t committedBits{};
        if (services && g_useTarget.committedTargetOffset >= 0) {
            std::memcpy(&committedBits, static_cast<const std::byte*>(services) + g_useTarget.committedTargetOffset,
                        sizeof(committedBits));
        }
        const auto committedHandle = std::bit_cast<std::int32_t>(committedBits);
        const auto target = selectedHandle != kInvalidHandle && selectedHandle == committedHandle
            ? selectedHandle
            : kInvalidHandle;
        callback(pawnHandle, target, true);
    } catch (...) {
        return polyhook::ResultType::Ignored;
    }
    return polyhook::ResultType::Ignored;
}

polyhook::ResultType HealthshotCanUsePre(polyhook::HookHandle,
                                         polyhook::ParametersHandle parameters,
                                         std::int32_t,
                                         polyhook::ReturnHandle result,
                                         polyhook::CallbackType type) noexcept {
    const auto callback = g_healthshot.canUseCallback;
    if (type != polyhook::CallbackType::Pre || !callback || !parameters || !result) {
        return polyhook::ResultType::Ignored;
    }
    try {
        auto* const weapon = polyhook::GetArgument<void*>(parameters, 0);
        auto* const pawn = polyhook::GetArgument<void*>(parameters, 1);
        std::int32_t pawnHandle = kInvalidHandle;
        std::int32_t weaponHandle = kInvalidHandle;
        if (!ResolveHealthshotOwner(weapon, pawn, pawnHandle, weaponHandle) ||
            callback(pawnHandle, weaponHandle) == 0) {
            return polyhook::ResultType::Ignored;
        }
        polyhook::SetReturn<bool>(result, false);
        return polyhook::ResultType::Supercede;
    } catch (...) {
        return polyhook::ResultType::Ignored;
    }
}

polyhook::ResultType HealthshotEffectPost(polyhook::HookHandle,
                                          polyhook::ParametersHandle parameters,
                                          std::int32_t,
                                          polyhook::ReturnHandle,
                                          polyhook::CallbackType type) noexcept {
    const auto callback = g_healthshot.effectCallback;
    if (type != polyhook::CallbackType::Post || !callback || !parameters) {
        return polyhook::ResultType::Ignored;
    }
    try {
        auto* const weapon = polyhook::GetArgument<void*>(parameters, 0);
        auto* const pawn = polyhook::GetArgument<void*>(parameters, 1);
        std::int32_t pawnHandle = kInvalidHandle;
        std::int32_t weaponHandle = kInvalidHandle;
        if (ResolveHealthshotOwner(weapon, pawn, pawnHandle, weaponHandle)) {
            callback(pawnHandle, weaponHandle);
        }
    } catch (...) {
        return polyhook::ResultType::Ignored;
    }
    return polyhook::ResultType::Ignored;
}

polyhook::ResultType ChickenFollowUsePre(polyhook::HookHandle,
                                         polyhook::ParametersHandle parameters,
                                         std::int32_t,
                                         polyhook::ReturnHandle,
                                         polyhook::CallbackType type) noexcept {
    if (type != polyhook::CallbackType::Pre || !g_chickenFollowUse.active || !parameters) {
        return polyhook::ResultType::Ignored;
    }
    try {
        auto* const chicken = polyhook::GetArgument<void*>(parameters, 0);
        if (!ShouldBlockChickenFollow(g_chickenFollowUse.active, CurrentChickenUseEntity(chicken))) {
            return polyhook::ResultType::Ignored;
        }
        return polyhook::ResultType::Supercede;
    } catch (...) {
        return polyhook::ResultType::Ignored;
    }
}

} // namespace

bool HasUseTargetSupport() noexcept {
    try {
        return HookImportsAvailable() && ResolveUseTargetMetadata() && Signature(kUseTargetSignature);
    } catch (...) {
        return false;
    }
}

bool HookUseTarget(UseTargetCallback callback) noexcept {
    if (!callback || (g_useTarget.owner && g_useTarget.owner != callback)) {
        return false;
    }
    g_useTarget.owner = callback;
    try {
        return InstallUseTargetHook(callback);
    } catch (...) {
        (void)RemoveUseTargetHook();
        return false;
    }
}

bool UnhookUseTarget(UseTargetCallback callback) noexcept {
    if (!callback) {
        return false;
    }
    if (UseTargetEmpty()) {
        return true;
    }
    if (callback != g_useTarget.owner) {
        return false;
    }
    return RemoveUseTargetHook();
}

bool HasHealthshotSupport() noexcept {
    try {
        return HookImportsAvailable() && Signature(kHealthshotCanUseSignature) &&
               Signature(kHealthshotEffectSignature);
    } catch (...) {
        return false;
    }
}

bool HookHealthshot(HealthshotCanUseCallback canUse, HealthshotEffectCallback effect) noexcept {
    if (!canUse || !effect ||
        (g_healthshot.canUseOwner && g_healthshot.canUseOwner != canUse) ||
        (g_healthshot.effectOwner && g_healthshot.effectOwner != effect)) {
        return false;
    }
    g_healthshot.canUseOwner = canUse;
    g_healthshot.effectOwner = effect;
    try {
        return InstallHealthshotHooks(canUse, effect);
    } catch (...) {
        (void)RemoveHealthshotHooks();
        return false;
    }
}

bool UnhookHealthshot(HealthshotCanUseCallback canUse, HealthshotEffectCallback effect) noexcept {
    if (!canUse || !effect) {
        return false;
    }
    if (HealthshotEmpty()) {
        return true;
    }
    if (canUse != g_healthshot.canUseOwner || effect != g_healthshot.effectOwner) {
        return false;
    }
    return RemoveHealthshotHooks();
}

bool HasChickenFollowBlockSupport() noexcept {
    try {
        return ChickenFollowImportsAvailable() && Signature(kChickenFollowUseSignature);
    } catch (...) {
        return false;
    }
}

bool SetChickenFollowBlocking(bool active) noexcept {
    if (!active) {
        return RemoveChickenFollowUseHook();
    }
    if (!ChickenFollowUseEmpty() && !g_chickenFollowUse.active && !RemoveChickenFollowUseHook()) {
        return false;
    }
    try {
        if (InstallChickenFollowUseHook()) {
            return true;
        }
    } catch (...) {
    }
    g_chickenFollowUse.active = false;
    (void)RemoveChickenFollowUseHook();
    return false;
}

bool ShutdownInteractions() noexcept {
    const bool useClean = RemoveUseTargetHook();
    const bool healthshotClean = RemoveHealthshotHooks();
    const bool chickenFollowClean = RemoveChickenFollowUseHook();
    return useClean && healthshotClean && chickenFollowClean;
}

} // namespace funrounds::native::detail

extern "C" PLUGIN_API bool HasUseTargetSupport() {
    return funrounds::native::detail::HasUseTargetSupport();
}

extern "C" PLUGIN_API bool HookUseTarget(funrounds::native::detail::UseTargetCallback callback) {
    return funrounds::native::detail::HookUseTarget(callback);
}

extern "C" PLUGIN_API bool UnhookUseTarget(funrounds::native::detail::UseTargetCallback callback) {
    return funrounds::native::detail::UnhookUseTarget(callback);
}

extern "C" PLUGIN_API bool HasHealthshotSupport() {
    return funrounds::native::detail::HasHealthshotSupport();
}

extern "C" PLUGIN_API bool HookHealthshot(funrounds::native::detail::HealthshotCanUseCallback canUse,
                                          funrounds::native::detail::HealthshotEffectCallback effect) {
    return funrounds::native::detail::HookHealthshot(canUse, effect);
}

extern "C" PLUGIN_API bool UnhookHealthshot(funrounds::native::detail::HealthshotCanUseCallback canUse,
                                            funrounds::native::detail::HealthshotEffectCallback effect) {
    return funrounds::native::detail::UnhookHealthshot(canUse, effect);
}

extern "C" PLUGIN_API bool HasChickenFollowBlockSupport() {
    return funrounds::native::detail::HasChickenFollowBlockSupport();
}

extern "C" PLUGIN_API bool SetChickenFollowBlocking(bool active) {
    return funrounds::native::detail::SetChickenFollowBlocking(active);
}
