// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#include "ghost_radar.hpp"

#include <array>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <limits>
#include <mutex>
#include <utility>

#include <cstrike15/bot/bot.h>
#include <cstrike15/bot/cs_bot_manager.h>
#include <entity2/entityinstance.h>
#include <polyhook/polyhook.hpp>
#include <plugin_export.h>
#include <plg/plugin.hpp>
#include <s2sdk/s2sdk.hpp>

#include "engine.hpp"
#include "ghost_passive_noise_policy.hpp"
#include "ghost_visibility.hpp"

namespace funrounds::native::detail {
namespace {

constexpr char kBotManagerVTable[] = "CCSBotManager";
constexpr char kBotManagerFootstepHookName[] = "FunRounds::CCSBotManager::OnPlayerFootstep";
constexpr char kBotPassiveNoiseSignature[] = "FunRounds::CCSBotPassiveNoiseDispatch";
constexpr char kBotPassiveNoiseHookName[] = "FunRounds::CCSBotPassiveNoiseDispatch";
constexpr std::size_t kScopeCapacity = 64;

struct PassiveNoiseHookState {
    void* managerAddress{};
    polyhook::HookHandle managerHook{};
    bool ownsManagerHook{};
    void* noiseAddress{};
    polyhook::HookHandle noiseHook{};
    std::atomic<bool> ready{};
};

PassiveNoiseHookState g_state;
ghost_radar::SendInvocationGate g_scopeGate;
std::atomic<std::uint32_t> g_activeCallbacks{};
std::mutex g_quiescenceMutex;
std::condition_variable g_quiescenceCondition;
thread_local ghost_passive_noise::PlayerFootstepScope<kScopeCapacity> g_footstepScope;
thread_local bool g_footstepRootAdmitted{};

polyhook::ResultType PlayerFootstepScopeCallback(polyhook::HookHandle,
                                                  polyhook::ParametersHandle,
                                                  std::int32_t,
                                                  polyhook::ReturnHandle,
                                                  polyhook::CallbackType type) noexcept;
polyhook::ResultType BotPassiveNoiseCallback(polyhook::HookHandle,
                                              polyhook::ParametersHandle,
                                              std::int32_t,
                                              polyhook::ReturnHandle,
                                              polyhook::CallbackType type) noexcept;

class ActiveCallback final {
public:
    ActiveCallback() noexcept { g_activeCallbacks.fetch_add(1, std::memory_order_acq_rel); }
    ~ActiveCallback() {
        g_activeCallbacks.fetch_sub(1, std::memory_order_acq_rel);
        g_quiescenceCondition.notify_all();
    }
};

void LogNoiseInitializationFailure(const char* message) noexcept {
    try {
        if (plg::Log != nullptr) {
            plg::Log(message, plg::Severity::Error, plg::source_location::current());
        }
    } catch (...) {
    }
}

const char* MissingRequiredImport() noexcept {
    if (__polyhook_GetVirtualIndex == nullptr) {
        return "Ghost passive noise unavailable: missing PolyHook GetVirtualIndex import";
    }
    if (__polyhook_HookDetour == nullptr) {
        return "Ghost passive noise unavailable: missing PolyHook HookDetour import";
    }
    if (__polyhook_FindDetour == nullptr) {
        return "Ghost passive noise unavailable: missing PolyHook FindDetour import";
    }
    if (__polyhook_AddCallback == nullptr) {
        return "Ghost passive noise unavailable: missing PolyHook AddCallback import";
    }
    if (__polyhook_IsCallbackRegistered == nullptr) {
        return "Ghost passive noise unavailable: missing PolyHook IsCallbackRegistered import";
    }
    if (__polyhook_AreCallbacksRegistered == nullptr) {
        return "Ghost passive noise unavailable: missing PolyHook AreCallbacksRegistered import";
    }
    if (__polyhook_RemoveCallback == nullptr) {
        return "Ghost passive noise unavailable: missing PolyHook RemoveCallback import";
    }
    if (__polyhook_UnhookDetour == nullptr) {
        return "Ghost passive noise unavailable: missing PolyHook UnhookDetour import";
    }
    if (__polyhook_GetArgumentPointer == nullptr) {
        return "Ghost passive noise unavailable: missing PolyHook GetArgumentPointer import";
    }
    if (__s2sdk_GetGameConfigSignature == nullptr) {
        return "Ghost passive noise unavailable: missing S2SDK GetGameConfigSignature import";
    }
    if (__s2sdk_GetGameConfigVTable == nullptr) {
        return "Ghost passive noise unavailable: missing S2SDK GetGameConfigVTable import";
    }
    return nullptr;
}

std::uintptr_t EventIdentity(const IGameEvent* event) noexcept {
    // The identity is compared only while this event's synchronous manager call
    // is open. Do not dereference or retain the borrowed event pointer.
    return reinterpret_cast<std::uintptr_t>(event);
}

void EnterPlayerFootstepScope(std::uintptr_t eventIdentity) noexcept {
    bool admitted = false;
    if (g_footstepScope.Empty()) {
        g_footstepRootAdmitted = g_scopeGate.TryEnterRoot();
        admitted = g_footstepRootAdmitted && g_state.ready.load(std::memory_order_acquire);
    } else {
        admitted = g_footstepScope.Active();
    }
    g_footstepScope.Begin(admitted, eventIdentity);
}

void LeavePlayerFootstepScope(std::uintptr_t eventIdentity) noexcept {
    (void)g_footstepScope.End(eventIdentity);
    if (!g_footstepScope.Empty()) {
        return;
    }
    if (g_footstepRootAdmitted) {
        g_footstepRootAdmitted = false;
        g_scopeGate.LeaveRoot();
        g_quiescenceCondition.notify_all();
    }
}

polyhook::ResultType PlayerFootstepScopeCallback(polyhook::HookHandle,
                                                  polyhook::ParametersHandle parameters,
                                                  std::int32_t,
                                                  polyhook::ReturnHandle,
                                                  polyhook::CallbackType type) noexcept {
    ActiveCallback active;
    std::uintptr_t eventIdentity = 0;
    try {
        auto* const event = parameters == nullptr ? nullptr : polyhook::GetArgument<IGameEvent*>(parameters, 1);
        eventIdentity = EventIdentity(event);
    } catch (...) {
        // Keep the Pre/Post frame balanced even if the optional parameter read
        // fails. Identity zero fails open in the helper policy.
        eventIdentity = 0;
    }
    if (type == polyhook::CallbackType::Pre) {
        EnterPlayerFootstepScope(eventIdentity);
    } else if (type == polyhook::CallbackType::Post) {
        LeavePlayerFootstepScope(eventIdentity);
    }
    return polyhook::ResultType::Ignored;
}

polyhook::ResultType BotPassiveNoiseCallback(polyhook::HookHandle,
                                              polyhook::ParametersHandle parameters,
                                              std::int32_t,
                                              polyhook::ReturnHandle,
                                              polyhook::CallbackType type) noexcept {
    ActiveCallback active;
    if (type != polyhook::CallbackType::Pre || !g_state.ready.load(std::memory_order_acquire) ||
        !g_footstepScope.Active() || parameters == nullptr) {
        return polyhook::ResultType::Ignored;
    }

    try {
        auto* const bot = polyhook::GetArgument<CBot*>(parameters, 0);
        auto* const event = polyhook::GetArgument<IGameEvent*>(parameters, 1);
        auto* const sourcePawn = polyhook::GetArgument<CEntityInstance*>(parameters, 2);
        const auto scopeEventIdentity = g_footstepScope.ActiveEventIdentity();
        const auto helperEventIdentity = EventIdentity(event);
        if (bot == nullptr || event == nullptr || sourcePawn == nullptr || bot->m_pController == nullptr) {
            return polyhook::ResultType::Ignored;
        }

        const auto sourceRefHandle = sourcePawn->GetRefEHandle();
        const std::uint32_t sourceFullHandle = sourceRefHandle.ToInt();
        const std::int32_t sourceEntityIndex = sourceRefHandle.GetEntryIndex();
        auto* const controller = reinterpret_cast<CEntityInstance*>(bot->m_pController);
        const auto controllerRefHandle = controller->GetRefEHandle();
        const std::uint32_t controllerFullHandle = controllerRefHandle.ToInt();
        const std::int32_t controllerEntityIndex = controllerRefHandle.GetEntryIndex();
        if (sourceFullHandle == UINT32_MAX || controllerFullHandle == UINT32_MAX || controllerEntityIndex <= 0) {
            return polyhook::ResultType::Ignored;
        }

        const auto snapshot = CurrentGhostRadarSnapshot();
        const auto hiddenTargets = CurrentGhostVisibilityTargets();
        if (!snapshot || !hiddenTargets) {
            return polyhook::ResultType::Ignored;
        }

        const std::int32_t recipientSlot = controllerEntityIndex - 1;
        if (!ghost_passive_noise::ShouldSuppressPlayerFootstep(
                scopeEventIdentity,
                helperEventIdentity,
                sourceFullHandle,
                sourceEntityIndex,
                controllerFullHandle,
                recipientSlot,
                snapshot->recipientControllerHandles,
                snapshot->recipientUserIds,
                snapshot->connectedRecipientMask,
                snapshot->targets,
                *hiddenTargets)) {
            return polyhook::ResultType::Ignored;
        }
        return polyhook::ResultType::Supercede;
    } catch (...) {
        return polyhook::ResultType::Ignored;
    }
}

bool RemoveNoiseCallback(polyhook::HookHandle hook,
                         polyhook::CallbackType type,
                         polyhook::CallbackHandler callback) noexcept {
    try {
        if (hook == nullptr || !polyhook::IsCallbackRegistered(hook, type, callback)) {
            return true;
        }
        (void)polyhook::RemoveCallback(hook, type, callback);
        return !polyhook::IsCallbackRegistered(hook, type, callback);
    } catch (...) {
        return false;
    }
}

bool CloseScopeAdmissionAndWait() noexcept {
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(250);
    for (;;) {
        if (g_scopeGate.TryCloseWhenIdle() || g_scopeGate.IsClosed()) {
            return true;
        }
        try {
            std::unique_lock lock(g_quiescenceMutex);
            if (!g_quiescenceCondition.wait_until(lock, deadline, [] {
                    return g_scopeGate.ActiveRoots() == 0;
                })) {
                return false;
            }
        } catch (...) {
            return false;
        }
    }
}

bool WaitForCallbackDrain() noexcept {
    try {
        std::unique_lock lock(g_quiescenceMutex);
        return g_quiescenceCondition.wait_for(lock, std::chrono::milliseconds(250), [] {
            return g_activeCallbacks.load(std::memory_order_acquire) == 0 && g_scopeGate.ActiveRoots() == 0;
        });
    } catch (...) {
        return false;
    }
}

bool DetachNoiseHook() noexcept {
    try {
        if (g_state.noiseAddress == nullptr) {
            return g_state.noiseHook == nullptr;
        }
        const auto current = polyhook::FindDetour(g_state.noiseAddress);
        if (current == nullptr) {
            g_state.noiseAddress = nullptr;
            g_state.noiseHook = nullptr;
            return true;
        }
        if (g_state.noiseHook == nullptr || current != g_state.noiseHook) {
            return false;
        }
        if (polyhook::AreCallbacksRegistered(current)) {
            g_state.noiseAddress = nullptr;
            g_state.noiseHook = nullptr;
            return true;
        }
        if (!polyhook::UnhookDetour(g_state.noiseAddress) || polyhook::FindDetour(g_state.noiseAddress) != nullptr) {
            return false;
        }
        g_state.noiseAddress = nullptr;
        g_state.noiseHook = nullptr;
        return true;
    } catch (...) {
        return false;
    }
}

bool DetachManagerHook() noexcept {
    if (g_state.managerAddress == nullptr) {
        return g_state.managerHook == nullptr && !g_state.ownsManagerHook;
    }
    try {
        const auto current = polyhook::FindDetour(g_state.managerAddress);
        if (current == nullptr) {
            g_state.managerAddress = nullptr;
            g_state.managerHook = nullptr;
            g_state.ownsManagerHook = false;
            return true;
        }
        if (g_state.managerHook == nullptr || current != g_state.managerHook) {
            return false;
        }
        if (!g_state.ownsManagerHook || polyhook::AreCallbacksRegistered(current)) {
            // Preserve a hook another plugin already owned or adopted after our callback was removed.
            g_state.managerAddress = nullptr;
            g_state.managerHook = nullptr;
            g_state.ownsManagerHook = false;
            return true;
        }
        if (!polyhook::UnhookDetour(g_state.managerAddress) ||
            polyhook::FindDetour(g_state.managerAddress) != nullptr) {
            return false;
        }
        g_state.managerAddress = nullptr;
        g_state.managerHook = nullptr;
        g_state.ownsManagerHook = false;
        return true;
    } catch (...) {
        return false;
    }
}

bool RemoveAllCallbacks() noexcept {
    const bool noiseClean =
        RemoveNoiseCallback(g_state.noiseHook, polyhook::CallbackType::Pre, BotPassiveNoiseCallback);
    const bool managerPreClean =
        RemoveNoiseCallback(g_state.managerHook, polyhook::CallbackType::Pre, PlayerFootstepScopeCallback);
    const bool managerPostClean =
        RemoveNoiseCallback(g_state.managerHook, polyhook::CallbackType::Post, PlayerFootstepScopeCallback);
    return noiseClean && managerPreClean && managerPostClean;
}

bool InstallNoiseHook(const char*& failure) {
    if (g_state.noiseAddress != nullptr || g_state.noiseHook != nullptr) {
        failure = "Ghost passive noise unavailable: updater hook state was already populated";
        return false;
    }
    g_state.noiseAddress = s2sdk::GetGameConfigSignature(GameConfig(), kBotPassiveNoiseSignature);
    if (g_state.noiseAddress == nullptr) {
        failure = "Ghost passive noise unavailable: gamedata could not resolve CCSBot passive updater signature";
        return false;
    }
    if (polyhook::FindDetour(g_state.noiseAddress) != nullptr) {
        g_state.noiseAddress = nullptr;
        failure = "Ghost passive noise unavailable: CCSBot passive updater address already has a detour";
        return false;
    }

    const plg::vector<polyhook::DataType> arguments{
        polyhook::DataType::Pointer,
        polyhook::DataType::Pointer,
        polyhook::DataType::Pointer,
#if defined(_WIN32)
        polyhook::DataType::Float,
        polyhook::DataType::Int32,
        polyhook::DataType::Int32,
        polyhook::DataType::Int32,
#else
        polyhook::DataType::Int32,
        polyhook::DataType::Float,
        polyhook::DataType::Int64,
        polyhook::DataType::Int64,
#endif
        polyhook::DataType::Pointer,
    };
    g_state.noiseHook = polyhook::HookDetour(g_state.noiseAddress,
                                             polyhook::DataType::Void,
                                             arguments,
                                             -1,
                                             plg::string(kBotPassiveNoiseHookName));
    if (g_state.noiseHook == nullptr) {
        failure = "Ghost passive noise unavailable: PolyHook could not attach the CCSBot passive updater detour";
        return false;
    }
    (void)polyhook::AddCallback(g_state.noiseHook, polyhook::CallbackType::Pre, BotPassiveNoiseCallback);
    if (!polyhook::IsCallbackRegistered(g_state.noiseHook,
                                        polyhook::CallbackType::Pre,
                                        BotPassiveNoiseCallback)) {
        failure = "Ghost passive noise unavailable: updater detour Pre callback did not register";
        return false;
    }
    return true;
}

bool InstallPlayerFootstepScopeHook(const char*& failure) {
    if (g_state.managerHook != nullptr || g_state.managerAddress != nullptr || g_state.ownsManagerHook) {
        failure = "Ghost passive noise unavailable: CCSBotManager hook state was already populated";
        return false;
    }
    auto* const managerVTable = s2sdk::GetGameConfigVTable(GameConfig(), kBotManagerVTable);
    if (managerVTable == nullptr) {
        failure = "Ghost passive noise unavailable: gamedata could not resolve CCSBotManager vtable";
        return false;
    }

    auto onPlayerFootstep = &CCSBotManager::OnPlayerFootstep;
    auto lastManagerMethod = &CCSBotManager::OnServerShutdown;
    const auto footstepIndex = polyhook::GetVirtualIndex(reinterpret_cast<void*&>(onPlayerFootstep));
    const auto lastManagerIndex = polyhook::GetVirtualIndex(reinterpret_cast<void*&>(lastManagerMethod));
    if (footstepIndex < 0 || lastManagerIndex < footstepIndex) {
        failure = "Ghost passive noise unavailable: PolyHook could not resolve CCSBotManager virtual method indices";
        return false;
    }

    auto** const managerVTableEntries = static_cast<void**>(managerVTable);
    auto* const managerAddress = managerVTableEntries[footstepIndex];
    if (managerAddress == nullptr) {
        failure = "Ghost passive noise unavailable: CCSBotManager OnPlayerFootstep vtable entry was null";
        return false;
    }
    g_state.managerAddress = managerAddress;

    g_state.managerHook = polyhook::FindDetour(g_state.managerAddress);
    if (g_state.managerHook == nullptr) {
        const std::array arguments{polyhook::DataType::Pointer, polyhook::DataType::Pointer};
        g_state.managerHook = polyhook::HookDetour(g_state.managerAddress,
                                                    polyhook::DataType::Void,
                                                    plg::vector<polyhook::DataType>(arguments.begin(), arguments.end()),
                                                    -1,
                                                    plg::string(kBotManagerFootstepHookName));
        if (g_state.managerHook == nullptr) {
            g_state.managerHook = polyhook::FindDetour(g_state.managerAddress);
            if (g_state.managerHook == nullptr) {
                g_state.managerAddress = nullptr;
                failure = "Ghost passive noise unavailable: PolyHook could not attach CCSBotManager OnPlayerFootstep detour";
                return false;
            }
        } else {
            g_state.ownsManagerHook = true;
        }
    }

    (void)polyhook::AddCallback(g_state.managerHook,
                                polyhook::CallbackType::Pre,
                                PlayerFootstepScopeCallback);
    (void)polyhook::AddCallback(g_state.managerHook,
                                polyhook::CallbackType::Post,
                                PlayerFootstepScopeCallback);
    const bool preRegistered = polyhook::IsCallbackRegistered(g_state.managerHook,
                                                              polyhook::CallbackType::Pre,
                                                              PlayerFootstepScopeCallback);
    const bool postRegistered = polyhook::IsCallbackRegistered(g_state.managerHook,
                                                               polyhook::CallbackType::Post,
                                                               PlayerFootstepScopeCallback);
    if (!preRegistered || !postRegistered) {
        failure = "Ghost passive noise unavailable: CCSBotManager OnPlayerFootstep Pre/Post callback registration failed";
        return false;
    }
    return true;
}

} // namespace

bool InitializeGhostPassiveNoise() noexcept {
    if (g_state.ready.load(std::memory_order_acquire)) {
        return true;
    }
    if (const char* const missingImport = MissingRequiredImport(); missingImport != nullptr) {
        LogNoiseInitializationFailure(missingImport);
        return false;
    }
    if (g_state.managerHook != nullptr || g_state.managerAddress != nullptr || g_state.ownsManagerHook ||
        g_state.noiseAddress != nullptr || g_state.noiseHook != nullptr) {
        LogNoiseInitializationFailure("Ghost passive noise unavailable: native hook state was already populated");
        return false;
    }
    const char* failure = "Ghost passive noise unavailable: initialization threw an exception";
    try {
        if (!EnsureEngine()) {
            LogNoiseInitializationFailure("Ghost passive noise unavailable: Source2 engine/game config initialization failed");
            return false;
        }
        if (!g_scopeGate.Open()) {
            LogNoiseInitializationFailure("Ghost passive noise unavailable: callback scope gate could not be opened");
            return false;
        }
        if (!InstallNoiseHook(failure) || !InstallPlayerFootstepScopeHook(failure)) {
            const bool cleanupSucceeded = ShutdownGhostPassiveNoise();
            LogNoiseInitializationFailure(failure);
            if (!cleanupSucceeded) {
                LogNoiseInitializationFailure("Ghost passive noise initialization rollback could not be verified");
            }
            return false;
        }
        g_state.ready.store(true, std::memory_order_release);
        return true;
    } catch (...) {
        const bool cleanupSucceeded = ShutdownGhostPassiveNoise();
        LogNoiseInitializationFailure(failure);
        if (!cleanupSucceeded) {
            LogNoiseInitializationFailure("Ghost passive noise exception rollback could not be verified");
        }
        return false;
    }
}

bool GhostPassiveNoiseReady() noexcept {
    return g_state.ready.load(std::memory_order_acquire);
}

bool ShutdownGhostPassiveNoise() noexcept {
    g_state.ready.store(false, std::memory_order_release);
    if (!CloseScopeAdmissionAndWait() || !RemoveAllCallbacks() || !WaitForCallbackDrain() || !DetachNoiseHook() ||
        !DetachManagerHook()) {
        return false;
    }
    g_state.ready.store(false, std::memory_order_release);
    return true;
}

} // namespace funrounds::native::detail

extern "C" PLUGIN_API bool HasGhostPassiveNoiseSupport() {
    return funrounds::native::detail::InitializeGhostPassiveNoise();
}
