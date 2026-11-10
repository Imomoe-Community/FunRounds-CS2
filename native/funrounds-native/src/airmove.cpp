// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#include "airmove.hpp"

#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <thread>

#include <entity2/entityinstance.h>
#include <entity2/entitynetwork.h>
#include <plugin_export.h>
#include <polyhook/polyhook.hpp>
#include <s2sdk/s2sdk.hpp>

#include "engine.hpp"
#include "entity_handle.hpp"

namespace funrounds::native::detail {
namespace {

constexpr char kAirMove[] = "FunRounds::AirMove";
constexpr std::uint64_t kDirections = (1ULL << 3) | (1ULL << 4) | (1ULL << 9) | (1ULL << 10);
using Assist = bool (*)(std::int32_t, std::int32_t, std::int32_t, float, float, float, float&, float&);
using Phase = polyhook::CallbackType;
using Result = polyhook::ResultType;

struct MoveLayout {
    std::size_t yaw{}, forward{}, left{}, vx{}, vy{};
};

struct AirMoveState {
    void* address{};
    polyhook::HookHandle hook{};
    MoveLayout layout{};
    SchemaField movementOwner{};
    SchemaField pawnController{};
    SchemaField pawnLifeState{};
    SchemaField pawnHealth{};
    Assist assist{};
    std::thread::id gameThread{};
} g_state;

struct Edit {
    std::byte* move{};
    float forward{}, left{}, writtenForward{}, writtenLeft{};
    unsigned nested{};
};
thread_local Edit g_edit;

template <typename T>
T Read(const std::byte* object, std::size_t offset) noexcept {
    T value;
    std::memcpy(&value, object + offset, sizeof(value));
    return value;
}

void WriteMoveAxis(std::byte* move, std::size_t offset, float value) noexcept {
    std::memcpy(move + offset, &value, sizeof(value));
}

bool ResolveMoveLayout(const std::byte* entry, MoveLayout& layout) {
    const auto config = GameConfig();
    const auto body = [&](const char* signature, const char* fromEntry) -> const std::byte* {
        auto* site = static_cast<const std::byte*>(s2sdk::GetGameConfigSignature(config, signature));
        const auto relative = s2sdk::GetGameConfigOffset(config, fromEntry);
        if (!site || relative <= 0 ||
            reinterpret_cast<std::uintptr_t>(site) - reinterpret_cast<std::uintptr_t>(entry) != static_cast<std::uintptr_t>(relative)) {
            return nullptr;
        }
        return site;
    };
    const auto* call = body("FunRounds::AirMove::AnglesCall", "FunRounds::AirMove::AnglesCallFromEntry");
    const auto* input = body("FunRounds::AirMove::InputLoads", "FunRounds::AirMove::InputLoadsFromEntry");
    const auto* velocity = body("FunRounds::AirMove::VelocityLoads", "FunRounds::AirMove::VelocityLoadsFromEntry");
    const auto* angles = static_cast<const std::byte*>(s2sdk::GetGameConfigSignature(config, "FunRounds::AirMove::AngleVectors"));
    if (!call || !input || !velocity || !angles) return false;

    // Gamedata provides the locations of operands inside the short signatures.
    // Keep operand reads bounded; a missing key is reported by S2SDK as -1.
    constexpr int maxOperandOffset = 63;
    const auto operand = [&](const char* key) {
        const auto offset = s2sdk::GetGameConfigOffset(config, key);
        return offset >= 0 && offset <= maxOperandOffset ? offset : -1;
    };
    const auto rel32 = operand("FunRounds::AirMove::AngleVectorsRel32");
    const auto base = operand("FunRounds::AirMove::AnglesBaseDisp8");
    const auto yaw = operand("FunRounds::AirMove::YawDisp8");
    const auto forward = operand("FunRounds::AirMove::ForwardDisp8");
    const auto left = operand("FunRounds::AirMove::LeftDisp8");
    const auto vx = operand("FunRounds::AirMove::VelocityXDisp8");
    const auto vy = operand("FunRounds::AirMove::VelocityYDisp8");
    if (rel32 < 0 || base < 0 || yaw < 0 || forward < 0 || left < 0 || vx < 0 || vy < 0) return false;

    const auto* relativeCall = call + rel32;
    const auto destination = reinterpret_cast<std::intptr_t>(relativeCall + sizeof(std::int32_t)) + Read<std::int32_t>(relativeCall, 0);
    if (destination != reinterpret_cast<std::intptr_t>(angles)) return false;

    const auto displacement = [](const std::byte* site, int index) -> std::size_t {
        return Read<std::uint8_t>(site, static_cast<std::size_t>(index));
    };
    // A packed load uses the same operand for both halves of the float pair.
    layout = {
        displacement(call, base) + displacement(angles, yaw),
        displacement(input, forward),
        displacement(input, left) + (left == forward ? sizeof(float) : 0),
        displacement(velocity, vx),
        displacement(velocity, vy) + (vy == vx ? sizeof(float) : 0),
    };
    const std::array offsets{layout.yaw, layout.forward, layout.left, layout.vx, layout.vy};
    for (std::size_t i = 0; i < offsets.size(); ++i) {
        if (offsets[i] == 0 || offsets[i] > 127 || offsets[i] % alignof(float) != 0) return false;
        for (std::size_t j = 0; j < i; ++j) {
            if (offsets[i] == offsets[j]) return false;
        }
    }
    return true;
}

bool ResolveAirMove() {
    if (g_state.address) {
        return true;
    }
    if (!__polyhook_HookDetour || !__polyhook_FindDetour || !__polyhook_UnhookDetour ||
        !__polyhook_AddCallback || !__polyhook_RemoveCallback || !__polyhook_IsCallbackRegistered ||
        !__polyhook_AreCallbacksRegistered || !__polyhook_GetArgumentPointer ||
        !__s2sdk_GetGameConfigSignature || !__s2sdk_GetGameConfigOffset || !__s2sdk_GetClientPawn || !__s2sdk_GetMaxClients ||
        !__s2sdk_PlayerSlotToEntHandle || !__s2sdk_IsClientInGame ||
        !__s2sdk_GetClientFlags || !__s2sdk_GetClientButtons || !__s2sdk_GetEntityMoveType ||
        !EnsureEngine() ||
        !ResolvePlayerPawnComponentChain(g_state.movementOwner) ||
        !ResolveSchemaField("CBasePlayerPawn", "m_hController", sizeof(std::uint32_t), g_state.pawnController) ||
        !ResolveSchemaField("CBaseEntity", "m_lifeState", sizeof(std::uint8_t), g_state.pawnLifeState) ||
        !ResolveSchemaField("CBaseEntity", "m_iHealth", sizeof(std::int32_t), g_state.pawnHealth)) {
        return false;
    }
    auto* entry = static_cast<std::byte*>(s2sdk::GetGameConfigSignature(GameConfig(), kAirMove));
    if (!entry || !ResolveMoveLayout(entry, g_state.layout)) {
        return false;
    }
    g_state.address = entry;
    return true;
}

Result AirMove(polyhook::HookHandle, polyhook::ParametersHandle args, std::int32_t,
               polyhook::ReturnHandle, Phase phase) noexcept {
    if (std::this_thread::get_id() != g_state.gameThread) {
        return Result::Ignored;
    }
    if (phase == Phase::Post) {
        if (g_edit.nested) {
            --g_edit.nested;
        } else if (auto* move = g_edit.move) {
            // Only undo our own writes. A cooperating hook may have replaced either axis.
            if (Read<float>(move, g_state.layout.forward) == g_edit.writtenForward) WriteMoveAxis(move, g_state.layout.forward, g_edit.forward);
            if (Read<float>(move, g_state.layout.left) == g_edit.writtenLeft) WriteMoveAxis(move, g_state.layout.left, g_edit.left);
            g_edit = {};
        }
        return Result::Ignored;
    }
    if (g_edit.move) {
        ++g_edit.nested;
        return Result::Ignored;
    }

    try {
        if (!g_state.assist) return Result::Ignored;
        auto* move = static_cast<std::byte*>(polyhook::GetArgument<void*>(args, 1));
        auto* services = static_cast<std::byte*>(polyhook::GetArgument<void*>(args, 0));
        if (!services || !move) return Result::Ignored;
        const float oldForward = Read<float>(move, g_state.layout.forward);
        const float oldLeft = Read<float>(move, g_state.layout.left);
        if (!(std::abs(oldForward) <= .001F && std::abs(oldLeft) <= .001F)) return Result::Ignored;
        auto* chain = reinterpret_cast<CNetworkVarChainer*>(services + g_state.movementOwner.offset);
        auto* pawn = reinterpret_cast<std::byte*>(chain->GetObject());
        if (!pawn) return Result::Ignored;
        const auto controllerHandle = ToApiHandle(Read<std::uint32_t>(pawn, g_state.pawnController.offset));
        const auto pawnHandle = HandleFromEntity(pawn);
        auto* controller = static_cast<CEntityInstance*>(EntityFromHandle(controllerHandle));
        if (!controller || pawnHandle == -1) return Result::Ignored;
        const auto slot = controller->GetRefEHandle().GetEntryIndex() - 1;
        if (slot < 0 || slot >= s2sdk::GetMaxClients() || !s2sdk::IsClientInGame(slot) ||
            s2sdk::PlayerSlotToEntHandle(slot) != controllerHandle || s2sdk::GetClientPawn(slot) != pawn) {
            return Result::Ignored;
        }
        // IsClientAlive follows the slot's original pawn, not the pawn being driven after BOT takeover.
        if (Read<std::uint8_t>(pawn, g_state.pawnLifeState.offset) != 0 ||
            Read<std::int32_t>(pawn, g_state.pawnHealth.offset) <= 0 || (s2sdk::GetClientFlags(slot) & 1) != 0 ||
            s2sdk::GetEntityMoveType(pawnHandle) != s2sdk::MoveType::Walk ||
            (s2sdk::GetClientButtons(slot, 0) & kDirections) != 0) {
            return Result::Ignored;
        }

        float forward{}, left{};
        if (g_state.assist(slot, pawnHandle, controllerHandle,
                           Read<float>(move, g_state.layout.yaw),
                           Read<float>(move, g_state.layout.vx),
                           Read<float>(move, g_state.layout.vy), forward, left)) {
            g_edit = {move, oldForward, oldLeft, forward, left, 0};
            WriteMoveAxis(move, g_state.layout.forward, forward);
            WriteMoveAxis(move, g_state.layout.left, left);
        }
    } catch (...) {
        // A failed read leaves the native movement command untouched.
    }
    return Result::Ignored;
}

bool RemoveAirMove() {
    g_state.assist = nullptr;
    if (!g_state.hook) return true;
    if (g_edit.move) return false;
    const auto hook = polyhook::FindDetour(g_state.address);
    if (hook && hook != g_state.hook) return false;
    if (hook) {
        for (const auto phase : {Phase::Pre, Phase::Post}) {
            if (polyhook::IsCallbackRegistered(hook, phase, AirMove)) {
                (void)polyhook::RemoveCallback(hook, phase, AirMove);
                if (polyhook::IsCallbackRegistered(hook, phase, AirMove)) return false;
            }
        }
        if (!polyhook::AreCallbacksRegistered(hook) &&
            !polyhook::UnhookDetour(g_state.address) && polyhook::FindDetour(g_state.address) == hook) {
            return false;
        }
    }
    g_state.hook = nullptr;
    return true;
}

bool SetAirMove(bool active, Assist assist) {
    if (!active) {
        return (!g_state.assist || g_state.assist == assist) && RemoveAirMove();
    }
    if (!assist || !ResolveAirMove()) return false;
    if (g_state.hook) return g_state.assist == assist;
    if (polyhook::FindDetour(g_state.address)) return false;
    using T = polyhook::DataType;
    g_state.hook = polyhook::HookDetour(g_state.address, T::Void, {T::Pointer, T::Pointer}, -1, kAirMove);
    if (!g_state.hook) return false;
    g_state.gameThread = std::this_thread::get_id();
    if (!polyhook::AddCallback(g_state.hook, Phase::Post, AirMove) ||
        !polyhook::AddCallback(g_state.hook, Phase::Pre, AirMove)) {
        (void)RemoveAirMove();
        return false;
    }
    g_state.assist = assist;
    return true;
}

} // namespace

bool ShutdownAirMove() noexcept {
    try {
        if (!RemoveAirMove()) return false;
        g_state = {};
        return true;
    } catch (...) {
        return false;
    }
}

} // namespace funrounds::native::detail

extern "C" PLUGIN_API bool HasAirMoveSupport() noexcept {
    try {
        return funrounds::native::detail::ResolveAirMove();
    } catch (...) {
        return false;
    }
}

extern "C" PLUGIN_API bool SetAirMoveAssist(bool active, funrounds::native::detail::Assist assist) noexcept {
    try {
        return funrounds::native::detail::SetAirMove(active, assist);
    } catch (...) {
        return false;
    }
}
