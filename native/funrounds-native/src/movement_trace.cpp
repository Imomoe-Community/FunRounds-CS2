// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#include "movement_trace.hpp"

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>

#if defined(_WIN32)
#include <Windows.h>
#elif defined(__linux__)
#include <dlfcn.h>
#include <elf.h>
#else
#error "Player movement hull tracing is supported on Windows x64 and Linux x64 only"
#endif

#include <cmodel.h>
#include <bspflags.h>
#include <const.h>
#include <entity2/entityinstance.h>
#include <gametrace.h>
#include <plugin_export.h>
#include <s2sdk/s2sdk.hpp>

#include "engine.hpp"
#include "entity_handle.hpp"
#if defined(__linux__)
#include "linux_image.hpp"
#endif

namespace funrounds::native::detail {
namespace {

constexpr char kSignature[] = "FunRounds::PlayerMovementTraceShape";
constexpr std::int32_t kInvalidHandle = -1;
constexpr std::uint64_t kPlayersolidMask = 798737;

constexpr std::uintptr_t kWindowsTraceShapeRva = 0xC2F3C0;
constexpr std::uintptr_t kWindowsReceiverSlotRva = 0x1E1E260;
constexpr std::uintptr_t kWindowsReceiverRva = 0x218AE50;
constexpr std::uint32_t kWindowsPeTimestamp = 0x6AEBC501;
constexpr std::uint32_t kWindowsImageSize = 0x257C000;
constexpr std::uintptr_t kLinuxTraceShapeRva = 0x173B540;
constexpr std::uintptr_t kLinuxReceiverSlotRva = 0x27A47F0;
constexpr std::uintptr_t kLinuxReceiverRva = 0x29DAA00;
constexpr unsigned char kLinuxBuildId[] = {
    0x5E, 0xA0, 0xE4, 0x82, 0xB4, 0x6E, 0xB7, 0x1C, 0x4F, 0x96,
    0x15, 0xC4, 0x8B, 0x34, 0xDD, 0xC3, 0x6F, 0x55, 0xCA, 0x40,
};

using TraceShapeFn = bool (*)(void*, const Ray_t&, const Vector&, const Vector&, CTraceFilter*, CGameTrace*);

struct TraceSchema {
    SchemaField pawnCollision{};
    SchemaField pawnOwner{};
    SchemaField collisionAttributes{};
    SchemaField interactsAs{};
    SchemaField interactsExclude{};
    bool ready{};
};

struct Query {
    TraceShapeFn call{};
    void* receiver{};

    explicit operator bool() const noexcept {
        return call != nullptr && receiver != nullptr;
    }
};

struct QueryAddresses {
    TraceShapeFn call{};
    void** receiverSlot{};
    void* expectedReceiver{};

    explicit operator bool() const noexcept {
        return call != nullptr && receiverSlot != nullptr && expectedReceiver != nullptr;
    }
};

TraceSchema g_traceSchema;
QueryAddresses g_queryAddresses;

static_assert(sizeof(Ray_t) == 0x30);
static_assert(offsetof(Ray_t, m_eType) == 0x28);
static_assert(sizeof(CGameTrace) == 0xC0);
static_assert(offsetof(CGameTrace, m_pEnt) == 0x08);
static_assert(offsetof(CGameTrace, m_vStartPos) == 0x78);
static_assert(offsetof(CGameTrace, m_vEndPos) == 0x84);
static_assert(offsetof(CGameTrace, m_vHitNormal) == 0x90);
static_assert(offsetof(CGameTrace, m_flFraction) == 0xAC);
static_assert(offsetof(CGameTrace, m_eRayType) == 0xBA);
static_assert(offsetof(CGameTrace, m_bStartInSolid) == 0xBB);
static_assert(offsetof(CTraceFilter, m_nInteractsWith) == 0x08);
#if defined(_WIN32)
static_assert(sizeof(CTraceFilter) == 0x48);
static_assert(offsetof(CTraceFilter, m_bIterateEntities) == 0x40);
#else
// Itanium reuses RnQueryAttr_t tail padding; this is the observed Linux layout.
static_assert(sizeof(CTraceFilter) == 0x40);
static_assert(offsetof(CTraceFilter, m_bIterateEntities) == 0x3A);
#endif
static_assert(MASK_PLAYERSOLID == kPlayersolidMask);

bool Finite(const Vector& value) noexcept {
    return std::isfinite(value.x) && std::isfinite(value.y) && std::isfinite(value.z);
}

bool WithinImage(std::uintptr_t base, std::size_t imageSize, std::uintptr_t rva, std::size_t length) noexcept {
    return rva <= imageSize && length <= imageSize - rva && base <= std::numeric_limits<std::uintptr_t>::max() - rva;
}

#if defined(_WIN32)
bool CurrentImageBase(void* function, std::uintptr_t& base, std::size_t& imageSize) noexcept {
    MEMORY_BASIC_INFORMATION memory{};
    if (!function || VirtualQuery(function, &memory, sizeof(memory)) != sizeof(memory) ||
        memory.Type != MEM_IMAGE || !memory.AllocationBase) {
        return false;
    }

    base = reinterpret_cast<std::uintptr_t>(memory.AllocationBase);
    const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    if (dos->e_magic != IMAGE_DOS_SIGNATURE || dos->e_lfanew <= 0 || dos->e_lfanew > 0x1000) {
        return false;
    }

    const auto* nt = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + static_cast<std::uintptr_t>(dos->e_lfanew));
    if (nt->Signature != IMAGE_NT_SIGNATURE || nt->FileHeader.Machine != IMAGE_FILE_MACHINE_AMD64 ||
        nt->FileHeader.TimeDateStamp != kWindowsPeTimestamp || nt->OptionalHeader.SizeOfImage != kWindowsImageSize) {
        return false;
    }

    imageSize = nt->OptionalHeader.SizeOfImage;
    return true;
}
#else
bool MatchesLinuxBuildId(std::uintptr_t base, std::size_t& imageSize) noexcept {
    const auto* elf = reinterpret_cast<const Elf64_Ehdr*>(base);
    if (std::memcmp(elf->e_ident, ELFMAG, SELFMAG) != 0 || elf->e_ident[EI_CLASS] != ELFCLASS64 ||
        elf->e_ident[EI_DATA] != ELFDATA2LSB || elf->e_machine != EM_X86_64 ||
        elf->e_phentsize != sizeof(Elf64_Phdr) || elf->e_phnum == 0 || elf->e_phnum > 64 ||
        elf->e_phoff > 0x10000) {
        return false;
    }

    const auto* headers = reinterpret_cast<const Elf64_Phdr*>(base + elf->e_phoff);
    std::uint64_t mappedEnd{};
    // ELF may list PT_NOTE before PT_LOAD, so resolve the full image end first.
    if (!ComputeMappedImageEnd(headers, elf->e_phnum, mappedEnd)) {
        return false;
    }

    for (std::uint16_t index = 0; index < elf->e_phnum; ++index) {
        const Elf64_Phdr& header = headers[index];
        if (header.p_type != PT_NOTE || header.p_memsz < sizeof(Elf64_Nhdr) || header.p_memsz > 0x10000 ||
            base > std::numeric_limits<std::uintptr_t>::max() - header.p_vaddr) {
            continue;
        }

        const auto* cursor = reinterpret_cast<const unsigned char*>(base + header.p_vaddr);
        const auto* const limit = cursor + header.p_memsz;
        while (static_cast<std::size_t>(limit - cursor) >= sizeof(Elf64_Nhdr)) {
            const auto* note = reinterpret_cast<const Elf64_Nhdr*>(cursor);
            const std::size_t nameSize = (static_cast<std::size_t>(note->n_namesz) + 3U) & ~std::size_t{3U};
            const std::size_t descSize = (static_cast<std::size_t>(note->n_descsz) + 3U) & ~std::size_t{3U};
            cursor += sizeof(Elf64_Nhdr);
            if (nameSize > static_cast<std::size_t>(limit - cursor) ||
                descSize > static_cast<std::size_t>(limit - cursor) - nameSize) {
                break;
            }
            if (note->n_type == NT_GNU_BUILD_ID && note->n_namesz == 4 && note->n_descsz == sizeof(kLinuxBuildId) &&
                std::memcmp(cursor, "GNU", 4) == 0 &&
                std::memcmp(cursor + nameSize, kLinuxBuildId, sizeof(kLinuxBuildId)) == 0) {
                imageSize = static_cast<std::size_t>(mappedEnd);
                return true;
            }
            cursor += nameSize + descSize;
        }
    }
    return false;
}

bool CurrentImageBase(void* function, std::uintptr_t& base, std::size_t& imageSize) noexcept {
    Dl_info info{};
    if (!function || dladdr(function, &info) == 0 || !info.dli_fbase) {
        return false;
    }

    base = reinterpret_cast<std::uintptr_t>(info.dli_fbase);
    if (!MatchesLinuxBuildId(base, imageSize)) {
        return false;
    }
    return true;
}
#endif

Query ResolveQuery() noexcept {
    if (!g_queryAddresses) {
        if (!__s2sdk_GetGameConfigSignature || !EnsureEngine()) {
            return {};
        }

        try {
            void* const signature = s2sdk::GetGameConfigSignature(GameConfig(), kSignature);
            if (!signature) {
                return {};
            }

            std::uintptr_t base{};
            std::size_t imageSize{};
            if (!CurrentImageBase(signature, base, imageSize)) {
                return {};
            }

#if defined(_WIN32)
            constexpr std::uintptr_t functionRva = kWindowsTraceShapeRva;
            constexpr std::uintptr_t receiverSlotRva = kWindowsReceiverSlotRva;
            constexpr std::uintptr_t receiverRva = kWindowsReceiverRva;
#else
            constexpr std::uintptr_t functionRva = kLinuxTraceShapeRva;
            constexpr std::uintptr_t receiverSlotRva = kLinuxReceiverSlotRva;
            constexpr std::uintptr_t receiverRva = kLinuxReceiverRva;
#endif

            const auto signatureAddress = reinterpret_cast<std::uintptr_t>(signature);
            if (!WithinImage(base, imageSize, functionRva, 1) || signatureAddress != base + functionRva ||
                !WithinImage(base, imageSize, receiverSlotRva, sizeof(void*)) ||
                !WithinImage(base, imageSize, receiverRva, sizeof(void*))) {
                return {};
            }

            g_queryAddresses = {
                reinterpret_cast<TraceShapeFn>(signature),
                reinterpret_cast<void**>(base + receiverSlotRva),
                reinterpret_cast<void*>(base + receiverRva),
            };
        } catch (...) {
            return {};
        }
    }

    const QueryAddresses addresses = g_queryAddresses;
    // Cache only stable addresses; the receiver slot and its first field remain
    // live values and must pass these checks on every use.
    void* const receiver = *addresses.receiverSlot;
    if (receiver != addresses.expectedReceiver || !*reinterpret_cast<void**>(receiver)) {
        return {};
    }
    return {addresses.call, receiver};
}

bool ResolveTraceSchema() noexcept {
    if (g_traceSchema.ready) {
        return true;
    }

    TraceSchema resolved{};
    if (!ResolveSchemaField("CBaseEntity", "m_pCollision", sizeof(void*), resolved.pawnCollision) ||
        !ResolveSchemaField("CBaseEntity", "m_hOwnerEntity", sizeof(std::uint32_t), resolved.pawnOwner) ||
        !ResolveSchemaField("CCollisionProperty", "m_collisionAttribute", 0, resolved.collisionAttributes) ||
        !ResolveSchemaField("VPhysicsCollisionAttribute_t", "m_nInteractsAs", sizeof(std::uint64_t), resolved.interactsAs) ||
        !ResolveSchemaField("VPhysicsCollisionAttribute_t", "m_nInteractsExclude", sizeof(std::uint64_t), resolved.interactsExclude)) {
        return false;
    }

    g_traceSchema = resolved;
    g_traceSchema.ready = true;
    return true;
}

template <typename T>
T ReadField(const void* entity, std::int32_t offset) noexcept {
    T value{};
    if (entity && offset >= 0) {
        std::memcpy(&value, static_cast<const std::byte*>(entity) + offset, sizeof(T));
    }
    return value;
}

void InitializeMiss(const Vector& end,
                    Vector& outPosition,
                    float& outFraction,
                    bool& outHit,
                    std::int32_t& outEntityHandle,
                    bool& outStartSolid,
                    Vector& outNormal) noexcept {
    outPosition = end;
    outFraction = 1.0F;
    outHit = false;
    outEntityHandle = kInvalidHandle;
    outStartSolid = false;
    outNormal.Init(0.0F, 0.0F, 0.0F);
}

bool TracePlayerMovementHullImpl(std::int32_t pawnHandle,
                                 const Vector& start,
                                 const Vector& end,
                                 const Vector& hullMins,
                                 const Vector& hullMaxs,
                                 Vector& outPosition,
                                 float& outFraction,
                                 bool& outHit,
                                 std::int32_t& outEntityHandle,
                                 bool& outStartSolid,
                                 Vector& outNormal) {
    InitializeMiss(end, outPosition, outFraction, outHit, outEntityHandle, outStartSolid, outNormal);

    if (pawnHandle == kInvalidHandle || !Finite(start) || !Finite(end) || !Finite(hullMins) || !Finite(hullMaxs) ||
        hullMins.x > hullMaxs.x || hullMins.y > hullMaxs.y || hullMins.z > hullMaxs.z ||
        !__s2sdk_EntPointerToEntHandle || !__s2sdk_EntHandleToEntPointer || !IsPlayerPawn(pawnHandle)) {
        return false;
    }

    void* const pawn = EntityFromHandle(pawnHandle);
    if (!pawn || HandleFromEntity(pawn) != pawnHandle || EntityFromHandle(pawnHandle) != pawn) {
        return false;
    }
    if (!ResolveTraceSchema()) {
        return false;
    }

    void* const collision = ReadField<void*>(pawn, g_traceSchema.pawnCollision.offset);
    if (!collision) {
        return false;
    }
    const auto collisionAttributes = static_cast<const std::byte*>(collision) + g_traceSchema.collisionAttributes.offset;
    const auto interactsAs = ReadField<std::uint64_t>(collisionAttributes, g_traceSchema.interactsAs.offset);
    const auto interactsExclude = ReadField<std::uint64_t>(collisionAttributes, g_traceSchema.interactsExclude.offset);
    const auto ownerHandle = ToApiHandle(ReadField<std::uint32_t>(pawn, g_traceSchema.pawnOwner.offset));
    void* owner = ownerHandle == kInvalidHandle ? nullptr : EntityFromHandle(ownerHandle);
    if (owner && HandleFromEntity(owner) != ownerHandle) {
        owner = nullptr;
    }

    const Query query = ResolveQuery();
    if (!query) {
        return false;
    }

    Ray_t ray(hullMins, hullMaxs);
    CTraceFilter filter(kPlayersolidMask, COLLISION_GROUP_PLAYER_MOVEMENT, false);
    auto* const pawnEntity = static_cast<CEntityInstance*>(pawn);
    filter.SetPassEntity1(pawnEntity);
    filter.SetPassEntityOwner1(static_cast<CEntityInstance*>(owner));
    filter.m_nInteractsAs = interactsAs;
    filter.m_nInteractsExclude = interactsExclude;
    filter.m_bHitSolidRequiresGenerateContacts = true;

    CGameTrace trace{};
    (void)query.call(query.receiver, ray, start, end, &filter, &trace);
    if (!Finite(trace.m_vEndPos) || !Finite(trace.m_vHitNormal) || !std::isfinite(trace.m_flFraction) ||
        trace.m_flFraction < 0.0F || trace.m_flFraction > 1.0F) {
        return false;
    }

    outPosition = trace.m_vEndPos;
    outFraction = trace.m_flFraction;
    outHit = trace.DidHit();
    outStartSolid = trace.m_bStartInSolid;
    outNormal = trace.m_vHitNormal;
    if (outHit) {
        if (!trace.m_pEnt) {
            InitializeMiss(end, outPosition, outFraction, outHit, outEntityHandle, outStartSolid, outNormal);
            return false;
        }
        const std::int32_t hitHandle = HandleFromEntity(trace.m_pEnt);
        void* const resolved = hitHandle == kInvalidHandle ? nullptr : EntityFromHandle(hitHandle);
        if (resolved == trace.m_pEnt && HandleFromEntity(resolved) == hitHandle) {
            outEntityHandle = hitHandle;
        } else {
            InitializeMiss(end, outPosition, outFraction, outHit, outEntityHandle, outStartSolid, outNormal);
            return false;
        }
    }

    return true;
}

} // namespace

void ResetMovementTraceMetadata() noexcept {
    g_traceSchema = {};
    g_queryAddresses = {};
}

} // namespace funrounds::native::detail

extern "C" PLUGIN_API bool HasPlayerMovementHullSupport() {
    try {
        return static_cast<bool>(funrounds::native::detail::ResolveQuery());
    } catch (...) {
        return false;
    }
}

extern "C" PLUGIN_API bool TracePlayerMovementHull(std::int32_t pawnHandle,
                                                   const Vector& start,
                                                   const Vector& end,
                                                   const Vector& hullMins,
                                                   const Vector& hullMaxs,
                                                   Vector& outPosition,
                                                   float& outFraction,
                                                   bool& outHit,
                                                   std::int32_t& outEntityHandle,
                                                   bool& outStartSolid,
                                                   Vector& outNormal) {
    try {
        return funrounds::native::detail::TracePlayerMovementHullImpl(pawnHandle, start, end, hullMins, hullMaxs,
                                                                      outPosition, outFraction, outHit,
                                                                      outEntityHandle, outStartSolid, outNormal);
    } catch (...) {
        funrounds::native::detail::InitializeMiss(end, outPosition, outFraction, outHit, outEntityHandle,
                                                  outStartSolid, outNormal);
        return false;
    }
}
