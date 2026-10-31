// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later
#include "third_person.hpp"

#include <array>
#include <atomic>
#include <cstddef>
#include <cstring>
#include <map>
#include <memory>
#include <thread>

#include <polyhook/polyhook.hpp>
#include <plugin_export.h>
#include <s2sdk/s2sdk.hpp>

#include "engine.hpp"

namespace funrounds::native::detail {
namespace {
constexpr char kSignature[] = "FunRounds::ThirdPersonEyeCameraBranch";
using Targets = std::map<std::int32_t, std::int32_t>;
struct Snapshot {
    Targets targets;
    std::thread::id gameThread;
    SchemaField services{}, view{}, life{}, health{}, cameraPawn{};
};
struct HookState {
    void* address{};
    polyhook::HookHandle hook{};
};
HookState g_hook;
std::atomic<std::shared_ptr<const Snapshot>> g_snapshot;

template <typename T>
T Read(const void* object, SchemaField field) noexcept {
    T value{};
    if (object) {
        std::memcpy(&value, static_cast<const std::byte*>(object) + field.offset, sizeof(value));
    }
    return value;
}

bool ImportsAvailable() noexcept {
    return __polyhook_HookDetour2 && __polyhook_FindDetour && __polyhook_AddCallback &&
           __polyhook_RemoveCallback && __polyhook_IsCallbackRegistered && __polyhook_UnhookDetour &&
           __polyhook_GetRegisterPointer && __polyhook_SetRegisterPointer &&
           __s2sdk_GetGameConfigSignature && __s2sdk_GetEntityClassname;
}

bool ResolveSchema(Snapshot& state) noexcept {
    return EnsureEngine() &&
           ResolveSchemaField("CBasePlayerPawn", "m_pCameraServices", sizeof(void*), state.services) &&
           ResolveSchemaField("CPlayer_CameraServices", "m_hViewEntity", sizeof(std::int32_t), state.view) &&
           ResolveSchemaField("CBaseEntity", "m_lifeState", sizeof(std::uint8_t), state.life) &&
           ResolveSchemaField("CBaseEntity", "m_iHealth", sizeof(std::int32_t), state.health) &&
           ResolveSchemaField("CCSCustomPlayerCamera", "m_hPawn", sizeof(std::int32_t), state.cameraPawn);
}

void* BranchAddress(const Snapshot& schema) noexcept {
    auto* start = static_cast<std::byte*>(s2sdk::GetGameConfigSignature(GameConfig(), kSignature));
    if (!start) {
        return nullptr;
    }
    // IDA, CS2 server snapshots inspected 2026-10-28: this site is BEFORE TEST,
    // so trampoline TEST regenerates flags from the edited temporary register.
    // Neither ABI return values nor the entity's camera-services pointer change.
#if defined(_WIN32)
    constexpr std::ptrdiff_t siteOffset = 0x17;
    constexpr std::ptrdiff_t servicesOffset = 0x13;
    constexpr std::array<std::uint8_t, 8> instructions{0x48, 0x85, 0xC9, 0x74, 0x2C, 0x48, 0x8B, 0x01};
#elif defined(__linux__)
    constexpr std::ptrdiff_t siteOffset = 0x13;
    constexpr std::ptrdiff_t servicesOffset = 0x0F;
    constexpr std::array<std::uint8_t, 9> instructions{0x48, 0x85, 0xFF, 0x0F, 0x84, 0x84, 0x00, 0x00, 0x00};
#else
#error "Third-person eye origin requires Windows x64 or Linux x64"
#endif
    std::int32_t displacement{};
    std::memcpy(&displacement, start + servicesOffset, sizeof(displacement));
    auto* site = start + siteOffset;
    if (displacement != schema.services.offset || std::memcmp(site, instructions.data(), instructions.size()) != 0) {
        return nullptr;
    }
    return site;
}

bool OwnView(std::int32_t pawn, std::int32_t camera, const Snapshot& state) noexcept {
    void* const entity = EntityFromHandle(pawn);
    return entity && HandleFromEntity(entity) == pawn &&
           Read<std::int32_t>(Read<void*>(entity, state.services), state.view) == camera;
}

polyhook::ResultType EyePre(polyhook::HookHandle, polyhook::ParametersHandle parameters,
                           std::int32_t, polyhook::ReturnHandle, polyhook::CallbackType type) noexcept {
    try {
        const auto state = g_snapshot.load(std::memory_order_acquire);
        // Engine pointer/handle queries below belong to the registering game thread.
        // Worker invocations retain the native path; the minimal driver measures both.
        if (!parameters || type != polyhook::CallbackType::Pre || !state ||
            std::this_thread::get_id() != state->gameThread) {
            return polyhook::ResultType::Ignored;
        }
        const auto registers = static_cast<polyhook::RegistersHandle>(parameters);
#if defined(_WIN32)
        void* const pawn = polyhook::GetRegister<void*>(registers, polyhook::RegisterType::RDI);
        constexpr auto temporary = polyhook::RegisterType::RCX;
#else
        void* const pawn = polyhook::GetRegister<void*>(registers, polyhook::RegisterType::RBX);
        constexpr auto temporary = polyhook::RegisterType::RDI;
#endif
        const auto pawnHandle = HandleFromEntity(pawn);
        const auto target = state->targets.find(pawnHandle);
        if (target == state->targets.end() || EntityFromHandle(pawnHandle) != pawn ||
            !IsPlayerPawn(pawnHandle) || Read<std::uint8_t>(pawn, state->life) != 0 ||
            Read<std::int32_t>(pawn, state->health) <= 0) {
            return polyhook::ResultType::Ignored;
        }
        void* const camera = EntityFromHandle(target->second);
        void* const services = Read<void*>(pawn, state->services);
        if (!services || !camera || HandleFromEntity(camera) != target->second ||
            Read<std::int32_t>(camera, state->cameraPawn) != pawnHandle ||
            Read<std::int32_t>(services, state->view) != target->second ||
            polyhook::GetRegister<void*>(registers, temporary) != services) {
            return polyhook::ResultType::Ignored;
        }
        // Do not call GetEyePosition here: the original pawn branch computes XYZ.
        polyhook::SetRegister<void*>(registers, temporary, nullptr);
    } catch (...) {
        return polyhook::ResultType::Ignored;
    }
    return polyhook::ResultType::Ignored;
}

bool RemoveHook() noexcept {
    try {
        if (!g_hook.address) {
            return !g_hook.hook;
        }
        auto current = polyhook::FindDetour(g_hook.address);
        if (!current) {
            g_hook = {};
            return true;
        }
        if (current != g_hook.hook) {
            return false;
        }
        if (polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, EyePre)) {
            (void)polyhook::RemoveCallback(current, polyhook::CallbackType::Pre, EyePre);
            if (polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, EyePre)) {
                return false;
            }
        }
        (void)polyhook::UnhookDetour(g_hook.address);
        if (polyhook::FindDetour(g_hook.address)) {
            return false;
        }
        g_hook = {};
        return true;
    } catch (...) {
        return false;
    }
}

bool InstallHook(const Snapshot& schema) {
    if (g_hook.address) {
        return g_hook.hook && polyhook::FindDetour(g_hook.address) == g_hook.hook &&
               polyhook::IsCallbackRegistered(g_hook.hook, polyhook::CallbackType::Pre, EyePre);
    }
    void* const address = BranchAddress(schema);
    if (!address || polyhook::FindDetour(address)) {
        return false;
    }
    g_hook.address = address;
    g_hook.hook = polyhook::HookDetour2(address, kSignature);
    if (!g_hook.hook) {
        (void)RemoveHook();
        return false;
    }
    (void)polyhook::AddCallback(g_hook.hook, polyhook::CallbackType::Pre, EyePre);
    return polyhook::IsCallbackRegistered(g_hook.hook, polyhook::CallbackType::Pre, EyePre);
}
} // namespace

bool HasThirdPersonEyeSupport() noexcept {
    try {
        Snapshot schema;
        return ImportsAvailable() && ResolveSchema(schema) &&
               (g_hook.address ? (g_hook.hook && polyhook::FindDetour(g_hook.address) == g_hook.hook) : BranchAddress(schema) != nullptr);
    } catch (...) {
        return false;
    }
}

bool RegisterThirdPersonEyeTarget(std::int32_t pawn, std::int32_t camera) noexcept {
    try {
        if (!HasThirdPersonEyeSupport() || !IsPlayerPawn(pawn)) {
            return false;
        }
        auto previous = g_snapshot.load(std::memory_order_acquire);
        auto next = previous ? std::make_shared<Snapshot>(*previous) : std::make_shared<Snapshot>();
        if ((!previous && !ResolveSchema(*next)) || (previous && next->gameThread != std::this_thread::get_id())) {
            return false;
        }
        void* const entity = EntityFromHandle(camera);
        if (!entity || HandleFromEntity(entity) != camera ||
            s2sdk::GetEntityClassname(camera) != "custom_player_camera" ||
            Read<std::int32_t>(entity, next->cameraPawn) != pawn) {
            return false;
        }
        const auto found = next->targets.find(pawn);
        if (found != next->targets.end() && found->second != camera) {
            return false;
        }
        next->gameThread = std::this_thread::get_id();
        next->targets[pawn] = camera;
        if (!InstallHook(*next)) {
            if (!previous || previous->targets.empty()) {
                (void)RemoveHook();
            }
            return false;
        }
        g_snapshot.store(std::move(next), std::memory_order_release);
        return true;
    } catch (...) {
        return false;
    }
}

bool UnregisterThirdPersonEyeTarget(std::int32_t pawn, std::int32_t camera) noexcept {
    try {
        const auto previous = g_snapshot.load(std::memory_order_acquire);
        if (!previous) {
            return !g_hook.address || RemoveHook();
        }
        if (previous->gameThread != std::this_thread::get_id() || OwnView(pawn, camera, *previous)) {
            return false;
        }
        auto next = std::make_shared<Snapshot>(*previous);
        const auto found = next->targets.find(pawn);
        if (found != next->targets.end()) {
            if (found->second != camera) {
                return false;
            }
            next->targets.erase(found);
        }
        g_snapshot.store(next, std::memory_order_release);
        return !next->targets.empty() || RemoveHook();
    } catch (...) {
        return false;
    }
}

bool ResetThirdPersonEyeTargets() noexcept {
    g_snapshot.store(nullptr, std::memory_order_release);
    return RemoveHook();
}

bool ShutdownThirdPersonEye() noexcept {
    try {
        const auto state = g_snapshot.load(std::memory_order_acquire);
        if (state) {
            for (const auto& [pawn, camera] : state->targets) {
                if (OwnView(pawn, camera, *state)) {
                    return false;
                }
            }
        }
        return ResetThirdPersonEyeTargets();
    } catch (...) {
        return false;
    }
}
} // namespace funrounds::native::detail

extern "C" PLUGIN_API bool HasThirdPersonEyeSupport() {
    return funrounds::native::detail::HasThirdPersonEyeSupport();
}
extern "C" PLUGIN_API bool RegisterThirdPersonEyeTarget(std::int32_t pawn, std::int32_t camera) {
    return funrounds::native::detail::RegisterThirdPersonEyeTarget(pawn, camera);
}
extern "C" PLUGIN_API bool UnregisterThirdPersonEyeTarget(std::int32_t pawn, std::int32_t camera) {
    return funrounds::native::detail::UnregisterThirdPersonEyeTarget(pawn, camera);
}
extern "C" PLUGIN_API bool ResetThirdPersonEyeTargets() {
    return funrounds::native::detail::ResetThirdPersonEyeTargets();
}
