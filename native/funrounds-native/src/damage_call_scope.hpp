#pragma once

namespace funrounds::native {

template <typename Callback>
class DamageCallScope {
public:
    DamageCallScope(const void* info, Callback callback, bool skipRules) noexcept
        : info_(info), callback_(callback), skipRules_(skipRules), previous_(current_) {
        current_ = this;
    }

    ~DamageCallScope() { current_ = previous_; }

    DamageCallScope(const DamageCallScope&) = delete;
    DamageCallScope& operator=(const DamageCallScope&) = delete;

    static bool SkipRules(const void* info, Callback callback) noexcept {
        for (auto* scope = current_; scope; scope = scope->previous_) {
            if (scope->info_ == info && scope->callback_ == callback) {
                return scope->skipRules_;
            }
        }
        return false;
    }

private:
    const void* info_;
    Callback callback_;
    bool skipRules_;
    DamageCallScope* previous_;
    static inline thread_local DamageCallScope* current_ = nullptr;
};

} // namespace funrounds::native
