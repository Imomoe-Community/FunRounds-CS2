// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#include "ghost_radar.hpp"

#include <algorithm>
#include <array>
#include <atomic>
#include <bit>
#include <chrono>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <mutex>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <entity2/entityinstance.h>
#include <eiface.h>
#include <google/protobuf/descriptor.h>
#include <google/protobuf/message.h>
#include <interfaces/interfaces.h>
#include <networksystem/inetworkmessages.h>
#include <networksystem/netmessage.h>
#include <polyhook/polyhook.hpp>
#include <plugin_export.h>
#include <s2sdk/s2sdk.hpp>
#include <serversideclientbase.h>
#include <soundsystem/isoundopsystem.h>

#include "engine.hpp"
#include "ghost_passive_sound_catalog.hpp"
#include "ghost_passive_sound_policy.hpp"
#include "ghost_visibility.hpp"

namespace funrounds::native::detail {
namespace {

constexpr char kClientVTable[] = "CServerSideClient";
constexpr NetworkMessageId kSpottedEntityUpdateMessageId = 325;
// EBaseGameEvents::GE_SosStartSoundEvent in the paired SourceSDK gameevents.proto.
constexpr NetworkMessageId kSosStartSoundEventMessageId = 208;
constexpr std::string_view kBreathSoundEventName = "Hostage.Breath";

struct SosStartSoundEventFields {
    const google::protobuf::Descriptor* descriptor{};
    const google::protobuf::FieldDescriptor* sourceEntityIndex{};
    const google::protobuf::FieldDescriptor* soundEventHash{};
};

polyhook::ResultType SendNetMessageCallback(polyhook::HookHandle,
                                            polyhook::ParametersHandle,
                                            std::int32_t,
                                            polyhook::ReturnHandle,
                                            polyhook::CallbackType) noexcept;

struct GhostRadarHookState {
    polyhook::HookHandle hook{};
    void* clientVTable{};
    std::atomic<bool> ready{};
};

struct SendFrame {
    bool ownsReplacement{};
    CServerSideClientBase* client{};
    CNetMessage* originalMessage{};
    CNetMessage* replacementMessage{};
    INetworkSerializerPB* serializer{};
};

struct SpottedUpdateFields {
    const google::protobuf::FieldDescriptor* updates{};
    const google::protobuf::FieldDescriptor* entityIndex{};
};

struct SpottedUpdateView {
    const google::protobuf::Message* message{};
    const google::protobuf::FieldDescriptor* entityIndex{};

    std::int32_t entity_idx() const noexcept {
        return message->GetReflection()->GetInt32(*message, entityIndex);
    }
};

class RepeatedSpottedUpdates final {
public:
    RepeatedSpottedUpdates(google::protobuf::Message& message, const SpottedUpdateFields& fields) noexcept
        : message_(message), fields_(fields), reflection_(message.GetReflection()) {}

    int size() const noexcept { return reflection_->FieldSize(message_, fields_.updates); }

    SpottedUpdateView Get(int index) const {
        const auto& update = reflection_->GetRepeatedMessage(message_, fields_.updates, index);
        return {&update, fields_.entityIndex};
    }

    void DeleteSubrange(int index, int count) {
        for (int removed = 0; removed < count; ++removed) {
            const int last = size() - 1;
            for (int current = index; current < last; ++current) {
                reflection_->SwapElements(&message_, fields_.updates, current, current + 1);
            }
            reflection_->RemoveLast(&message_, fields_.updates);
        }
    }

private:
    google::protobuf::Message& message_;
    const SpottedUpdateFields& fields_;
    const google::protobuf::Reflection* reflection_;
};

GhostRadarHookState g_state;
INetworkMessages* g_networkMessages{};
IVEngineServer2* g_engineServer{};
ghost_radar::SendInvocationGate g_sendGate;
std::atomic<std::shared_ptr<const GhostRadarSnapshot>> g_snapshot;
SosStartSoundEventFields g_sosStartSoundFields;
std::atomic<std::shared_ptr<const std::vector<std::uint32_t>>> g_passiveSoundEventHashes;
std::atomic<bool> g_passiveSoundMessagesReady{};
std::atomic<std::uint32_t> g_activePreCallbacks{};
std::atomic<std::uint32_t> g_activePostCallbacks{};
std::atomic<std::uint32_t> g_pendingFrames{};
std::mutex g_quiescenceMutex;
std::condition_variable g_quiescenceCondition;
// PolyHook creates distinct ParametersSpan wrappers for Pre and Post; pair calls by same-thread LIFO instead.
thread_local ghost_radar::NestedInvocationStack<SendFrame, 64> g_sendFrames;
thread_local std::uint64_t g_sendCallDepth{};

bool RequiredImportsAvailable() noexcept {
    return __polyhook_FindVirtual2 != nullptr && __polyhook_AddCallback != nullptr &&
           __polyhook_IsCallbackRegistered != nullptr && __polyhook_RemoveCallback != nullptr &&
           __polyhook_GetArgumentPointer != nullptr && __polyhook_SetArgumentPointer != nullptr &&
           __polyhook_SetReturnBool != nullptr && __s2sdk_QueryInterface != nullptr &&
           __s2sdk_GetGameConfigVTable != nullptr && __s2sdk_GetMaxClients != nullptr &&
           __s2sdk_IsClientInGame != nullptr && __s2sdk_EntHandleToEntPointer != nullptr &&
           __s2sdk_IsEntityPlayerPawn != nullptr;
}

class ActiveCallback final {
public:
    explicit ActiveCallback(bool pre) noexcept : count_(pre ? g_activePreCallbacks : g_activePostCallbacks) {
        count_.fetch_add(1, std::memory_order_acq_rel);
    }

    ~ActiveCallback() {
        count_.fetch_sub(1, std::memory_order_acq_rel);
        g_quiescenceCondition.notify_all();
    }

private:
    std::atomic<std::uint32_t>& count_;
};

bool EnterSendInvocation() noexcept {
    if (g_sendCallDepth != 0) {
        ++g_sendCallDepth;
        return true;
    }
    if (!g_sendGate.TryEnterRoot()) {
        return false;
    }
    g_sendCallDepth = 1;
    return true;
}

void LeaveSendInvocation() noexcept {
    if (g_sendCallDepth == 0) {
        return;
    }
    --g_sendCallDepth;
    if (g_sendCallDepth == 0) {
        g_sendGate.LeaveRoot();
        g_quiescenceCondition.notify_all();
    }
}

bool CloseSendAdmissionAndWaitForSends() noexcept {
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(250);
    for (;;) {
        if (g_sendGate.TryCloseWhenIdle() || g_sendGate.IsClosed()) {
            return true;
        }

        try {
            std::unique_lock lock(g_quiescenceMutex);
            if (!g_quiescenceCondition.wait_until(lock, deadline, [] {
                    return g_sendGate.ActiveRoots() == 0;
                })) {
                return false;
            }
        } catch (...) {
            return false;
        }
    }
}

void ReleaseMessage(CNetMessage* message, INetworkSerializerPB* serializer) noexcept {
    if (message != nullptr && serializer != nullptr && g_networkMessages != nullptr) {
        g_networkMessages->DeallocateNetMessageAbstract(serializer, message);
    }
}

bool ResolveSpottedUpdateFields(const google::protobuf::Message& message,
                                SpottedUpdateFields& fields) noexcept {
    const auto* const descriptor = message.GetDescriptor();
    if (descriptor == nullptr) {
        return false;
    }
    const auto* const updates = descriptor->FindFieldByName("entity_updates");
    if (updates == nullptr || !updates->is_repeated() ||
        updates->cpp_type() != google::protobuf::FieldDescriptor::CPPTYPE_MESSAGE) {
        return false;
    }
    const auto* const entityIndex = updates->message_type()->FindFieldByName("entity_idx");
    if (entityIndex == nullptr || entityIndex->is_repeated() ||
        entityIndex->cpp_type() != google::protobuf::FieldDescriptor::CPPTYPE_INT32) {
        return false;
    }
    fields = {updates, entityIndex};
    return true;
}

bool ResolveSosStartSoundEventFields(const google::protobuf::Message& message,
                                     SosStartSoundEventFields& fields) noexcept {
    const auto* const descriptor = message.GetDescriptor();
    if (descriptor == nullptr) {
        return false;
    }
    const auto* const sourceEntityIndex = descriptor->FindFieldByName("source_entity_index");
    const auto* const soundEventHash = descriptor->FindFieldByName("soundevent_hash");
    if (sourceEntityIndex == nullptr || sourceEntityIndex->is_repeated() || sourceEntityIndex->number() != 3 ||
        sourceEntityIndex->type() != google::protobuf::FieldDescriptor::TYPE_INT32 ||
        soundEventHash == nullptr || soundEventHash->is_repeated() || soundEventHash->number() != 2 ||
        soundEventHash->type() != google::protobuf::FieldDescriptor::TYPE_FIXED32) {
        return false;
    }
    fields = {descriptor, sourceEntityIndex, soundEventHash};
    return true;
}

bool ReadSosStartSoundEvent(const google::protobuf::Message& message,
                            const SosStartSoundEventFields& fields,
                            std::int32_t& sourceEntityIndex,
                            std::uint32_t& soundEventHash) noexcept {
    if (fields.descriptor == nullptr || fields.sourceEntityIndex == nullptr || fields.soundEventHash == nullptr ||
        message.GetDescriptor() != fields.descriptor) {
        return false;
    }
    const auto* const reflection = message.GetReflection();
    if (reflection == nullptr || !reflection->HasField(message, fields.sourceEntityIndex) ||
        !reflection->HasField(message, fields.soundEventHash)) {
        return false;
    }
    sourceEntityIndex = reflection->GetInt32(message, fields.sourceEntityIndex);
    soundEventHash = reflection->GetUInt32(message, fields.soundEventHash);
    return true;
}

bool ShouldFilterPassiveSoundMessage(const CServerSideClientBase* client,
                                     const CNetMessage* message,
                                     const GhostRadarSnapshot& snapshot) noexcept {
    if (client == nullptr || message == nullptr ||
        !g_passiveSoundMessagesReady.load(std::memory_order_acquire)) {
        return false;
    }
    const auto hiddenTargets = CurrentGhostVisibilityTargets();
    const auto* const proto = message->AsMessage();
    if (!hiddenTargets || !proto) {
        return false;
    }

    std::int32_t sourceEntityIndex = -1;
    std::uint32_t soundEventHash = 0;
    if (!ReadSosStartSoundEvent(*proto, g_sosStartSoundFields, sourceEntityIndex, soundEventHash)) {
        return false;
    }

    const auto passiveSoundEventHashes = g_passiveSoundEventHashes.load(std::memory_order_acquire);
    if (!passiveSoundEventHashes) {
        return false;
    }

    const std::int32_t recipientSlot = client->GetPlayerSlot().Get();
    const std::int32_t liveUserId = client->GetUserID().Get();
    if (!ghost_radar::IsCurrentRecipient(recipientSlot, liveUserId, snapshot.recipientUserIds)) {
        return false;
    }

    return ghost_passive_sound::ShouldFilterStartSoundEvent(sourceEntityIndex,
                                                             soundEventHash,
                                                             recipientSlot,
                                                             *passiveSoundEventHashes,
                                                             snapshot.targets,
                                                             *hiddenTargets);
}

bool HasTargetedUpdate(const google::protobuf::Message& message,
                       const SpottedUpdateFields& fields,
                       std::int32_t recipientSlot,
                       const GhostRadarSnapshot& snapshot) noexcept {
    const auto* const reflection = message.GetReflection();
    const int updateCount = reflection->FieldSize(message, fields.updates);
    for (int index = 0; index < updateCount; ++index) {
        const auto& update = reflection->GetRepeatedMessage(message, fields.updates, index);
        const auto* const entityIndexReflection = update.GetReflection();
        const std::int32_t entityIndex = entityIndexReflection->GetInt32(update, fields.entityIndex);
        if (ghost_radar::ShouldFilterEntityUpdate(entityIndex, recipientSlot, snapshot.targets)) {
            return true;
        }
    }
    return false;
}

bool RemoveCallback(polyhook::CallbackType type) noexcept {
    try {
        if (g_state.hook == nullptr ||
            !polyhook::IsCallbackRegistered(g_state.hook, type, SendNetMessageCallback)) {
            return true;
        }
        (void)polyhook::RemoveCallback(g_state.hook, type, SendNetMessageCallback);
        return !polyhook::IsCallbackRegistered(g_state.hook, type, SendNetMessageCallback);
    } catch (...) {
        return false;
    }
}

polyhook::ResultType SuppressUnfilteredMessage(polyhook::ReturnHandle result) noexcept {
    if (result != nullptr) {
        polyhook::SetReturn<bool>(result, false);
        return polyhook::ResultType::Supercede;
    }
    return polyhook::ResultType::Ignored;
}

polyhook::ResultType SuppressUntrackedGhostMessage(polyhook::ParametersHandle parameters,
                                                   polyhook::ReturnHandle result) noexcept {
    if (parameters == nullptr || result == nullptr) {
        return polyhook::ResultType::Ignored;
    }

    try {
        const auto* const message = polyhook::GetArgument<const CNetMessage*>(parameters, 1);
        if (message == nullptr) {
            return SuppressUnfilteredMessage(result);
        }
        const auto messageId = message->GetMessageId();
        if (messageId == kSpottedEntityUpdateMessageId) {
            return SuppressUnfilteredMessage(result);
        }
        if (messageId != kSosStartSoundEventMessageId ||
            !g_passiveSoundMessagesReady.load(std::memory_order_acquire)) {
            return polyhook::ResultType::Ignored;
        }

        auto* const client = polyhook::GetArgument<CServerSideClientBase*>(parameters, 0);
        const auto snapshot = g_snapshot.load(std::memory_order_acquire);
        if (snapshot && ShouldFilterPassiveSoundMessage(client, message, *snapshot)) {
            return SuppressUnfilteredMessage(result);
        }
    } catch (...) {
    }
    return polyhook::ResultType::Ignored;
}

polyhook::ResultType SendNetMessagePre(polyhook::ParametersHandle parameters,
                                       polyhook::ReturnHandle result) noexcept {
    if (!EnterSendInvocation()) {
        return polyhook::ResultType::Ignored;
    }
    const auto invocationIndex = g_sendFrames.Begin();
    if (!invocationIndex) {
        return SuppressUntrackedGhostMessage(parameters, result);
    }
    if (parameters == nullptr || result == nullptr ||
        !g_state.ready.load(std::memory_order_acquire)) {
        return polyhook::ResultType::Ignored;
    }

    std::unique_ptr<CNetMessage, void (*)(CNetMessage*)> replacement(nullptr, [](CNetMessage* message) {
        if (message != nullptr) {
            ReleaseMessage(message, message->GetSerializerPB());
        }
    });

    try {
        auto* const client = polyhook::GetArgument<CServerSideClientBase*>(parameters, 0);
        const auto* const message = polyhook::GetArgument<const CNetMessage*>(parameters, 1);
        if (client == nullptr || message == nullptr) {
            return polyhook::ResultType::Ignored;
        }

        const auto messageId = message->GetMessageId();
        if (messageId == kSosStartSoundEventMessageId) {
            const auto snapshot = g_snapshot.load(std::memory_order_acquire);
            if (snapshot && ShouldFilterPassiveSoundMessage(client, message, *snapshot)) {
                return SuppressUnfilteredMessage(result);
            }
            return polyhook::ResultType::Ignored;
        }
        if (messageId != kSpottedEntityUpdateMessageId) {
            return polyhook::ResultType::Ignored;
        }

        const auto snapshot = g_snapshot.load(std::memory_order_acquire);
        if (!snapshot || g_networkMessages == nullptr) {
            return polyhook::ResultType::Ignored;
        }

        const std::int32_t recipientSlot = client->GetPlayerSlot().Get();
        const std::int32_t liveUserId = client->GetUserID().Get();
        if (!ghost_radar::IsCurrentRecipient(recipientSlot, liveUserId, snapshot->recipientUserIds)) {
            return polyhook::ResultType::Ignored;
        }

        if (!ghost_radar::HasTargetForRecipient(recipientSlot, snapshot->targets)) {
            return polyhook::ResultType::Ignored;
        }
        const auto* const originalProto = message->AsMessage();
        SpottedUpdateFields originalFields;
        if (originalProto == nullptr || !ResolveSpottedUpdateFields(*originalProto, originalFields)) {
            return SuppressUnfilteredMessage(result);
        }
        if (!HasTargetedUpdate(*originalProto, originalFields, recipientSlot, *snapshot)) {
            return polyhook::ResultType::Ignored;
        }

        INetworkSerializerPB* const serializer = message->GetSerializerPB();
        if (serializer == nullptr) {
            return SuppressUnfilteredMessage(result);
        }
        replacement.reset(g_networkMessages->AllocateAndCopyConstructNetMessageAbstract(serializer, message));
        if (!replacement) {
            return SuppressUnfilteredMessage(result);
        }

        const auto* const clonedProto = replacement.get()->AsMessage();
        SpottedUpdateFields clonedFields;
        if (clonedProto == nullptr || !ResolveSpottedUpdateFields(*clonedProto, clonedFields)) {
            return SuppressUnfilteredMessage(result);
        }
        auto* const mutableProto = const_cast<google::protobuf::Message*>(clonedProto);
        RepeatedSpottedUpdates updates(*mutableProto, clonedFields);
        if (ghost_radar::FilterEntityUpdates(updates, recipientSlot, snapshot->targets) == 0) {
            return SuppressUnfilteredMessage(result);
        }

        auto* const invocation = g_sendFrames.Get(*invocationIndex);
        if (invocation == nullptr) {
            return SuppressUnfilteredMessage(result);
        }
        *invocation = SendFrame{true,
                                client,
                                const_cast<CNetMessage*>(message),
                                replacement.get(),
                                serializer};
        replacement.release();
        g_pendingFrames.fetch_add(1, std::memory_order_acq_rel);
        polyhook::SetArgumentPointer(parameters, 1, invocation->replacementMessage);
        return polyhook::ResultType::Ignored;
    } catch (...) {
        return SuppressUnfilteredMessage(result);
    }
}

polyhook::ResultType SendNetMessagePost(polyhook::ParametersHandle parameters) noexcept {
    if (g_sendCallDepth == 0) {
        return polyhook::ResultType::Ignored;
    }

    SendFrame completed;
    if (g_sendFrames.End(completed) && completed.ownsReplacement) {
        if (parameters != nullptr) {
            try {
                auto* const client = polyhook::GetArgument<CServerSideClientBase*>(parameters, 0);
                auto* const message = polyhook::GetArgument<CNetMessage*>(parameters, 1);
                if (ghost_radar::MatchesSendInvocation(completed.client,
                                                      completed.replacementMessage,
                                                      client,
                                                      message)) {
                    polyhook::SetArgumentPointer(parameters, 1, completed.originalMessage);
                } else if (message == completed.replacementMessage) {
                    // A later pre-hook may retarget the client; do not leave our freed clone in arg1.
                    polyhook::SetArgumentPointer(parameters, 1, completed.originalMessage);
                }
            } catch (...) {
            }

        }

        ReleaseMessage(completed.replacementMessage, completed.serializer);
        g_pendingFrames.fetch_sub(1, std::memory_order_acq_rel);
        g_quiescenceCondition.notify_all();
    }

    LeaveSendInvocation();
    return polyhook::ResultType::Ignored;
}

polyhook::ResultType SendNetMessageCallback(polyhook::HookHandle,
                                            polyhook::ParametersHandle parameters,
                                            std::int32_t,
                                            polyhook::ReturnHandle result,
                                            polyhook::CallbackType type) noexcept {
    const bool pre = type == polyhook::CallbackType::Pre;
    ActiveCallback active(pre);
    if (pre) {
        return SendNetMessagePre(parameters, result);
    }
    return SendNetMessagePost(parameters);
}

bool RemoveCallbacksAfterSendDrain() noexcept {
    if (!CloseSendAdmissionAndWaitForSends() || !RemoveCallback(polyhook::CallbackType::Pre) ||
        !RemoveCallback(polyhook::CallbackType::Post)) {
        return false;
    }

    try {
        std::unique_lock lock(g_quiescenceMutex);
        return g_quiescenceCondition.wait_for(lock, std::chrono::milliseconds(250), [] {
            return g_activePreCallbacks.load(std::memory_order_acquire) == 0 &&
                   g_activePostCallbacks.load(std::memory_order_acquire) == 0 &&
                   g_pendingFrames.load(std::memory_order_acquire) == 0;
        });
    } catch (...) {
        return false;
    }
}

} // namespace

bool InitializeGhostRadarMessages() noexcept {
    if (g_state.ready.load(std::memory_order_acquire)) {
        return true;
    }
    if (!RequiredImportsAvailable() || !EnsureEngine()) {
        return false;
    }
    if (g_state.hook != nullptr || !g_sendGate.Open()) {
        return false;
    }

    try {
        g_networkMessages = static_cast<INetworkMessages*>(
            s2sdk::QueryInterface("networksystem", NETWORKMESSAGES_INTERFACE_VERSION));
        g_engineServer = static_cast<IVEngineServer2*>(
            s2sdk::QueryInterface("engine2", INTERFACEVERSION_VENGINESERVER));
        if (g_networkMessages == nullptr || g_engineServer == nullptr ||
            g_networkMessages->FindNetworkMessageById(kSpottedEntityUpdateMessageId) == nullptr) {
            g_networkMessages = nullptr;
            g_engineServer = nullptr;
            return false;
        }

        g_state.clientVTable = s2sdk::GetGameConfigVTable(GameConfig(), kClientVTable);
        if (g_state.clientVTable == nullptr) {
            return false;
        }

        auto sendNetMessage = &CServerSideClientBase::SendNetMessage;
        g_state.hook = polyhook::FindVirtual2(g_state.clientVTable, reinterpret_cast<void*&>(sendNetMessage));
        if (g_state.hook == nullptr) {
            return false;
        }

        (void)polyhook::AddCallback(g_state.hook, polyhook::CallbackType::Post, SendNetMessageCallback);
        if (!polyhook::IsCallbackRegistered(g_state.hook, polyhook::CallbackType::Post, SendNetMessageCallback)) {
            const bool postRemoved = RemoveCallback(polyhook::CallbackType::Post);
            if (postRemoved) {
                g_state.hook = nullptr;
                g_state.clientVTable = nullptr;
            }
            return false;
        }
        (void)polyhook::AddCallback(g_state.hook, polyhook::CallbackType::Pre, SendNetMessageCallback);
        if (!polyhook::IsCallbackRegistered(g_state.hook, polyhook::CallbackType::Pre, SendNetMessageCallback)) {
            const bool preRemoved = RemoveCallback(polyhook::CallbackType::Pre);
            const bool postRemoved = RemoveCallback(polyhook::CallbackType::Post);
            if (preRemoved && postRemoved) {
                g_state.hook = nullptr;
                g_state.clientVTable = nullptr;
            }
            return false;
        }

        g_state.ready.store(true, std::memory_order_release);
        return true;
    } catch (...) {
        const bool preRemoved = RemoveCallback(polyhook::CallbackType::Pre);
        const bool postRemoved = RemoveCallback(polyhook::CallbackType::Post);
        if (preRemoved && postRemoved) {
            g_state.hook = nullptr;
            g_state.clientVTable = nullptr;
            g_networkMessages = nullptr;
            g_engineServer = nullptr;
        }
        return false;
    }
}

bool GhostRadarMessagesReady() noexcept {
    return g_state.ready.load(std::memory_order_acquire);
}

bool GhostPassiveSoundMessagesReady() noexcept {
    return g_passiveSoundMessagesReady.load(std::memory_order_acquire);
}

bool ResolvePassiveSoundEventHashes(ISoundOpSystem& soundSystem, std::vector<std::uint32_t>& hashes) {
    std::uint32_t breathHash{};
    hashes.clear();
    hashes.reserve(ghost_passive_sound::kPassiveSoundEventCount);

    for (const auto& event : ghost_passive_sound::kSoundEventNames) {
        const std::string name(event.name);
        if (!soundSystem.HasSoundEvent(name.c_str())) {
            return false;
        }
        const std::uint32_t hash = soundSystem.GetSoundEventHash(name.c_str());
        if (!soundSystem.IsValidSoundEventHash(hash)) {
            return false;
        }
        const char* const roundTripName = soundSystem.GetSoundEventName(hash);
        if (roundTripName == nullptr || std::string_view(roundTripName) != event.name) {
            return false;
        }
        if (event.name == kBreathSoundEventName) {
            if (event.suppressForHiddenGhost) {
                return false;
            }
            breathHash = hash;
            continue;
        }
        if (!event.suppressForHiddenGhost) {
            return false;
        }
        hashes.push_back(hash);
    }

    if (!soundSystem.HasSoundEvent(kBreathSoundEventName.data())) {
        return false;
    }
    const std::uint32_t resolvedBreathHash = soundSystem.GetSoundEventHash(kBreathSoundEventName.data());
    const char* const resolvedBreathName = soundSystem.GetSoundEventName(resolvedBreathHash);
    if (breathHash != resolvedBreathHash || !soundSystem.IsValidSoundEventHash(resolvedBreathHash) ||
        resolvedBreathName == nullptr || std::string_view(resolvedBreathName) != kBreathSoundEventName) {
        return false;
    }

    std::sort(hashes.begin(), hashes.end());
    return hashes.size() == ghost_passive_sound::kPassiveSoundEventCount &&
           std::adjacent_find(hashes.begin(), hashes.end()) == hashes.end();
}

bool InitializeGhostPassiveSoundMessages() noexcept {
    if (g_passiveSoundMessagesReady.load(std::memory_order_acquire)) {
        return true;
    }
    if (!InitializeGhostRadarMessages() || g_networkMessages == nullptr) {
        return false;
    }

    try {
        auto* const soundSystem = static_cast<ISoundOpSystem*>(
            s2sdk::QueryInterface("soundsystem", SOUNDOPSYSTEM_INTERFACE_VERSION));
        if (soundSystem == nullptr) {
            return false;
        }
        std::vector<std::uint32_t> passiveSoundEventHashes;
        if (!ResolvePassiveSoundEventHashes(*soundSystem, passiveSoundEventHashes)) {
            return false;
        }

        INetworkSerializerPB* const serializer = g_networkMessages->FindNetworkMessageById(kSosStartSoundEventMessageId);
        if (serializer == nullptr) {
            return false;
        }
        CNetMessage* const sample = serializer->AllocateMessage();
        if (sample == nullptr) {
            return false;
        }

        SosStartSoundEventFields fields;
        bool valid = false;
        try {
            const auto* const proto = sample->AsMessage();
            valid = proto != nullptr && ResolveSosStartSoundEventFields(*proto, fields);
        } catch (...) {
            ReleaseMessage(sample, serializer);
            return false;
        }
        ReleaseMessage(sample, serializer);
        if (!valid) {
            return false;
        }

        g_sosStartSoundFields = fields;
        g_passiveSoundEventHashes.store(
            std::make_shared<const std::vector<std::uint32_t>>(std::move(passiveSoundEventHashes)),
            std::memory_order_release);
        g_passiveSoundMessagesReady.store(true, std::memory_order_release);
        return true;
    } catch (...) {
        return false;
    }
}

std::shared_ptr<const GhostRadarSnapshot> CurrentGhostRadarSnapshot() noexcept {
    return g_snapshot.load(std::memory_order_acquire);
}

void ForgetGhostRadarTargetsForMapEnd() noexcept {
    g_snapshot.store(nullptr, std::memory_order_release);
}

bool SetGhostRadarTargets(const plg::vector<std::int32_t>& fullPawnHandles,
                          const plg::vector<std::uint64_t>& enemyRecipientMasks) noexcept {
    if (fullPawnHandles.empty() && enemyRecipientMasks.empty()) {
        g_snapshot.store(nullptr, std::memory_order_release);
        return true;
    }
    if (!g_state.ready.load(std::memory_order_acquire) || !GhostRadarStateFilterReady() ||
        fullPawnHandles.size() != enemyRecipientMasks.size() ||
        g_engineServer == nullptr || !__s2sdk_GetMaxClients || !__s2sdk_IsClientInGame ||
        !__s2sdk_PlayerSlotToEntHandle) {
        return false;
    }

    try {
        std::vector<ghost_radar::Target> candidates;
        candidates.reserve(fullPawnHandles.size());
        for (std::size_t index = 0; index < fullPawnHandles.size(); ++index) {
            const std::int32_t handle = fullPawnHandles[index];
            if (handle == ghost_radar::kInvalidEntityHandle || !IsPlayerPawn(handle)) {
                return false;
            }
            void* const entity = EntityFromHandle(handle);
            if (entity == nullptr || HandleFromEntity(entity) != handle) {
                return false;
            }
            const auto refHandle = static_cast<CEntityInstance*>(entity)->GetRefEHandle();
            if (refHandle.ToInt() != std::bit_cast<std::uint32_t>(handle) || refHandle.GetEntryIndex() < 0) {
                return false;
            }
            candidates.push_back(ghost_radar::Target{std::bit_cast<std::uint32_t>(handle),
                                                     refHandle.GetEntryIndex(),
                                                     enemyRecipientMasks[index]});
        }

        auto next = std::make_shared<GhostRadarSnapshot>();
        next->recipientUserIds.fill(ghost_radar::kInvalidUserId);
        next->recipientControllerHandles.fill(UINT32_MAX);
        if (!ghost_radar::BuildTargetSet(candidates, next->targets)) {
            return false;
        }

        const std::int32_t maxClients = s2sdk::GetMaxClients();
        if (maxClients <= 0 || static_cast<std::size_t>(maxClients) > next->recipientUserIds.size()) {
            return false;
        }
        for (std::int32_t slot = 0; slot < maxClients; ++slot) {
            if (!s2sdk::IsClientInGame(slot)) {
                continue;
            }
            const std::int32_t controllerHandle = s2sdk::PlayerSlotToEntHandle(slot);
            if (controllerHandle == ghost_radar::kInvalidEntityHandle || !IsPlayerController(controllerHandle)) {
                continue;
            }
            void* const controller = EntityFromHandle(controllerHandle);
            if (controller == nullptr || HandleFromEntity(controller) != controllerHandle) {
                continue;
            }
            const auto controllerRefHandle = static_cast<CEntityInstance*>(controller)->GetRefEHandle();
            if (controllerRefHandle.ToInt() != std::bit_cast<std::uint32_t>(controllerHandle) ||
                controllerRefHandle.GetEntryIndex() != slot + 1) {
                continue;
            }
            const std::int32_t userId = g_engineServer->GetPlayerUserId(CPlayerSlot(slot)).Get();
            if (userId != ghost_radar::kInvalidUserId) {
                next->recipientUserIds[static_cast<std::size_t>(slot)] = userId;
                next->recipientControllerHandles[static_cast<std::size_t>(slot)] =
                    std::bit_cast<std::uint32_t>(controllerHandle);
                next->connectedRecipientMask |= UINT64_C(1) << static_cast<unsigned int>(slot);
            }
        }

        g_snapshot.store(std::move(next), std::memory_order_release);
        return true;
    } catch (...) {
        return false;
    }
}

bool ShutdownGhostRadar() noexcept {
    const bool passiveNoiseClean = ShutdownGhostPassiveNoise();
    const bool stateFilterClean = ShutdownGhostRadarStateFilter();
    g_passiveSoundMessagesReady.store(false, std::memory_order_release);
    g_state.ready.store(false, std::memory_order_release);
    g_snapshot.store(nullptr, std::memory_order_release);
    if (g_state.hook == nullptr) {
        g_sosStartSoundFields = {};
        g_passiveSoundEventHashes.store(nullptr, std::memory_order_release);
        g_networkMessages = nullptr;
        g_engineServer = nullptr;
        return passiveNoiseClean && stateFilterClean;
    }

    if (!RemoveCallbacksAfterSendDrain()) {
        return false;
    }
    if (!RemoveCallback(polyhook::CallbackType::Post)) {
        return false;
    }
    if (g_activePreCallbacks.load(std::memory_order_acquire) != 0 ||
        g_activePostCallbacks.load(std::memory_order_acquire) != 0 ||
        g_pendingFrames.load(std::memory_order_acquire) != 0) {
        return false;
    }
    g_state.hook = nullptr;
    g_state.clientVTable = nullptr;
    g_state.ready.store(false, std::memory_order_release);
    g_sosStartSoundFields = {};
    g_passiveSoundEventHashes.store(nullptr, std::memory_order_release);
    g_networkMessages = nullptr;
    g_engineServer = nullptr;
    return passiveNoiseClean && stateFilterClean;
}

} // namespace funrounds::native::detail

extern "C" PLUGIN_API bool HasGhostRadarSupport() {
    const bool messagesReady = funrounds::native::detail::InitializeGhostRadarMessages();
    const bool stateFilterReady = funrounds::native::detail::InitializeGhostRadarStateFilter();
    return messagesReady && stateFilterReady;
}

extern "C" PLUGIN_API bool HasGhostPassiveSoundMessageSupport() {
    return funrounds::native::detail::InitializeGhostPassiveSoundMessages();
}

extern "C" PLUGIN_API bool SetGhostRadarTargets(const plg::vector<std::int32_t>& fullPawnHandles,
                                               const plg::vector<std::uint64_t>& enemyRecipientMasks) {
    return funrounds::native::detail::SetGhostRadarTargets(fullPawnHandles, enemyRecipientMasks);
}
