# FunRounds-CS2

<div align="center">

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Rust Version](https://img.shields.io/badge/Rust-1.99.1+-orange.svg)](rust-toolchain.toml)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20x64-lightgrey.svg)]()
[![Plugify](https://img.shields.io/badge/Framework-Plugify%20%2B%20S2SDK%203.0.0-success.svg)](https://github.com/untrustedmodders/plugify)

**A high-performance, safe, and modular special rounds plugin for Counter-Strike 2 dedicated servers.**

[English](README_EN.md) • [简体中文 (README_ZH.md)](README_ZH.md) • [Bilingual (README.md)](README.md)

</div>

---

### Overview

**FunRounds-CS2** is a modern Counter-Strike 2 server plugin developed in Rust, built atop the [Plugify](https://github.com/untrustedmodders/plugify) framework and [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk). 

It revitalizes the classic "Fun Rounds" gameplay concept for Source 2. Each round (or upon administrative selection), the plugin seamlessly applies distinctive gameplay mutators—altering player attributes, loadouts, physics, combat rules, and bomb mechanics—while maintaining strict engine stability and zero-leak state restoration.

### Key Features

- **Modular Two-Crate Architecture**: 
  - `funrounds-core`: Pure, engine-agnostic Rust library handling state machines, round queues, math, rules, and strictly validated TOML configurations.
  - `funrounds-plugin`: Native Plugify plugin interacting safely with the CS2 engine on the game thread via S2SDK.
- **Rich Round Mutators**:
  - **Player Physics & Attributes**: Configurable health (up to 10,000), armor/helmets, gravity scaling, friction, auto-bunnyhop, blue visual overlay, and dynamic over-health decay.
  - **Recoil & Knockback**: Custom weapon recoil view-punch, view-punch decay, recoil/spread suppression, and shooting self-push / knockback physics (`fr_knock`).
  - **Weapons & Dynamic Loadouts**: Whitelisted CS2 weapons, infinite reserve or clip ammo, taser recharge timers, automatic grenade replenishment (HE & Decoy), independent random loadouts, and kill-streak weapon progression.
  - **Combat Modifiers**: Vampiric lifesteal, headshots-only, no-headshots, self-inflicted damage on missed shots, decoy dodgeball physics, and throwing knives with bouncing mechanics.
  - **Custom C4 / Bomb Mechanics**: Support for native bombs, chicken-carried bombs, portable carry bombs, custom plant/explode/defuse timers, and bouncing bomb physics.
- **Fail-Safe & Fault-Tolerant Design**:
  - **FFI Panic Containment**: All engine callbacks wrap in `catch_unwind` at the C ABI boundary to prevent server crashes.
  - **Transactional State Restoration**: Tracks player/round generations to guarantee only hijacked properties and ConVars are restored without corrupting native server settings.
  - **Dynamic Capability Quarantine**: Automatically excludes modes requiring missing SDK hooks or engine features without impacting other modes.

### Project Layout

```text
├── bindings/             # Plugify S2SDK provider bindings (123 methods)
├── config/               # Reference configurations (funrounds.example.toml)
├── crates/
│   ├── funrounds-core/   # Engine-independent gameplay logic, rules & config parser
│   └── funrounds-plugin/ # Native Plugify runtime plugin implementation
├── licenses/             # Third-party and dependency licenses
├── packaging/            # Plugin package manifest (funrounds.pplugin)
└── rust-toolchain.toml   # Fixed toolchain configuration (Rust 1.99.1)
```

### Installation

#### Prerequisites

- Counter-Strike 2 Dedicated Server (Windows x64 or Linux x64)
- [Plugify](https://github.com/untrustedmodders/plugify) installed on your CS2 server
- [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk) plugin (version `3.0.0`) active in Plugify

#### Deployment

1. **Deploy Plugin Files**:
   Place the compiled binary and manifest into the Plugify environments folder:
   - **Windows**:
     - `game/csgo/addons/plugify/envs/funrounds/bin/funrounds_plugin.dll`
     - `game/csgo/addons/plugify/envs/funrounds/funrounds.pplugin`
   - **Linux**:
     - `game/csgo/addons/plugify/envs/funrounds/bin/funrounds_plugin.so`
     - `game/csgo/addons/plugify/envs/funrounds/funrounds.pplugin`

2. **Configure Plugin**:
   Copy [config/funrounds.example.toml](config/funrounds.example.toml) to your server configuration directory:
   - Target path: `game/csgo/addons/plugify/configs/funrounds.toml`
   - Customize mode parameters as desired.

### Configuration

The configuration uses strict TOML format. Below is an abbreviated example:

```toml
# Top-level settings
diagnostics = false # Diagnostic logging (default: false)

[[modes]]
id = "example"              # Unique mode identifier (ASCII, alphanumeric, '_' or '-')
enabled = true              # Whether this mode enters the round pool
name = "Scout Jump"         # Display name
description = "Low gravity sniper battle"
weapons = ["weapon_ssg08", "weapon_glock"]

# Basic Attributes
health = 100                # Initial health (1-10000)
armor = 100                 # Initial armor (0-255)
gravity = 0.5               # Gravity multiplier (0.0 < g <= 10.0)

# Optional Mechanics
bunnyhop = true
no_spread_recoil = false
unlimited_reserve = true
lifesteal = false

# Optional Bomb Rules (Native, Chicken, or Carry)
# [modes.bomb]
# kind = "native"
# plant_time = 3.0
# explode_time = 45.0
# defuse_kit_time = 5.0
# defuse_time = 10.0
# bounce = false
```

### Commands Reference

#### Server & Administrator Commands (Console / RCON)

| Command | Arguments | Description |
| :--- | :--- | :--- |
| `fr_status` | None | Displays current mode, phase, engine capabilities, and fault diagnoses. |
| `fr_list` | None | Lists all loaded and enabled round modes in the pool. |
| `fr_next` | `<id \| random>` | Queues a specific mode ID or random mode for the next round. |
| `fr_reload` | None | Validates and schedules configuration reload for the next round. |
| `fr_enable` | `<0 \| 1>` | Master switch: disables (`0`) or enables (`1`) the plugin. |

#### Player Commands (Console & Chat)

| Console Command | Chat Alias | Description |
| :--- | :--- | :--- |
| `fr_info` | `!frinfo` | Displays the current round mode, ID, and description in chat. |
| `fr_help` | `!frhelp` | Displays available player commands and usage instructions. |
| `fr_knock` | `!knock [0\|1]` | Toggles or sets weapon recoil shooting self-push / knockback. |

### Building from Source

Ensure you have Rust `1.99.1` installed (or managed automatically via `rust-toolchain.toml`).

```bash
# Code formatting check
cargo fmt --all -- --check

# Lints and static analysis
cargo clippy --workspace --all-targets --locked -- -D warnings

# Execute test suite (119+ unit tests)
cargo test --workspace --locked

# Build optimized release binaries
cargo build --release --workspace --locked
```

Compiled libraries will be located in `target/release/`:
- Windows: `funrounds_plugin.dll`
- Linux: `libfunrounds_plugin.so` (rename to `funrounds_plugin.so` during deployment)

---

### License & Author

- **Author**: Mai Ooizumi ([@Mai-Ooizumi](https://github.com/Mai-Ooizumi))
- **License**: GNU Affero General Public License v3.0 or later ([AGPL-3.0-or-later](LICENSE))
