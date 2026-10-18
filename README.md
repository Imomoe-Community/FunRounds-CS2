# FunRounds-CS2

<div align="center">

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Rust Version](https://img.shields.io/badge/Rust-1.99.1+-orange.svg)](rust-toolchain.toml)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20x64-lightgrey.svg)]()
[![Plugify](https://img.shields.io/badge/Framework-Plugify%20%2B%20S2SDK%203.0.0-success.svg)](https://github.com/untrustedmodders/plugify)

**A high-performance, safe, and modular special rounds plugin for Counter-Strike 2 dedicated servers.**

[English](#english) • [简体中文](#简体中文) • [English Only (README_EN.md)](README_EN.md) • [中文独立文档 (README_ZH.md)](README_ZH.md)

</div>

---

<a name="english"></a>
## English

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

<a name="简体中文"></a>
## 简体中文

### 项目简介

**FunRounds-CS2** 是基于 Rust 语言开发、面向 Counter-Strike 2 专用服务端的特殊娱乐回合（Fun Rounds）插件。本项目构建于 [Plugify](https://github.com/untrustedmodders/plugify) 插件框架和 [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk) 扩展之上。

插件以现代化架构重构了经典 CS 特殊回合玩法。在每个回合开始时（或由管理员指定），插件能够无缝切换并应用独特的玩法模式——灵活修改玩家基础属性、配装、物理参数、战斗判定以及 C4 炸弹机制，同时保证严格的崩溃隔离与无残留状态恢复。

### 核心特性

- **双 Crate 模块化架构**：
  - `funrounds-core`：纯 Rust 编写、独立于游戏引擎的核心库。负责状态机流转、回合随机池、战斗数学规则以及严格校验的 TOML 配置解析。
  - `funrounds-plugin`：Plugify 原生适配插件。在游戏主线程通过 S2SDK 与 CS2 引擎交互，处理实体句柄追踪与生命周期管理。
- **丰富的玩法修改机制**：
  - **基础属性与物理参数**：支持自定义生命值（1～10000）、护甲头盔、重力倍率（`gravity`）、地面摩擦力（`friction`）、连跳（`bunnyhop`）、蓝色外观（`blue`）以及超额生命递减。
  - **后坐与镜头反馈**：支持自定义开火镜头震动（`recoil_view`）、镜头衰减（`view_punch_decay`）、散布与后坐抑制（`no_spread_recoil`）以及开火自身反冲推力（`fr_knock`）。
  - **武器与动态配装**：支持 CS2 官方武器白名单、无限备弹（`unlimited_reserve`）、无限弹匣（`unlimited_clip`）、电击枪充能间隔（`taser_charge`）、手雷自动补充（`replenish_he`/`replenish_decoy`）、随机武器抽取与击杀换武器（`kill_loadout`）。
  - **特殊战斗规则**：吸血恢复（`lifesteal`）、仅爆头伤害（`headshots_only`）、禁止爆头（`no_headshots`）、未命中自身扣血（`miss_damage`）、诱饵弹躲避球碰撞伤害（`dodgeball`）、右键飞刀及弹射判定（`throwing_knives`）以及团队死斗模式（`team_deathmatch`）。
  - **C4 特殊机制**：支持原生 C4（`native`）、鸡背 C4（`chicken`）、可携带 C4（`carry`），支持自定义安放时间、爆炸倒计时、拆除耗时以及炸弹弹跳物理。
- **高可靠性与故障隔离**：
  - **FFI Panic 拦截**：在 C ABI 边界全面捕获 Rust panic（`catch_unwind`），防止单次回调异常波及专用服务端进程。
  - **事务性状态恢复**：严格维护代次（Generation）与实体句柄，在回合结束或停用时仅还原由本插件修改过的属性和 ConVar，不污染服务端原有配置。
  - **能力动态隔离**：根据服务端运行环境的 SDK 钩子支持情况，自动隔离缺失底层能力的模式，其余模式不受影响正常运行。

### 目录结构

```text
├── bindings/             # Plugify S2SDK 接口清单 (123 项接口定义)
├── config/               # 示例配置文件 (funrounds.example.toml)
├── crates/
│   ├── funrounds-core/   # 独立于引擎的玩法核心、规则校验与配置解析
│   └── funrounds-plugin/ # 游戏线程 Plugify 插件运行时实现
├── licenses/             # 第三方依赖与开源协议文件
├── packaging/            # Plugify 插件打包清单 (funrounds.pplugin)
└── rust-toolchain.toml   # 固定编译器工具链版本 (Rust 1.99.1)
```

### 安装与部署

#### 前置要求

- Counter-Strike 2 专用服务端（Windows x64 或 Linux x64）
- 服务端已安装配置 [Plugify](https://github.com/untrustedmodders/plugify) 插件框架
- 服务端已加载并运行 [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk) 插件（版本 `3.0.0`）

#### 部署步骤

1. **部署插件文件**：
   将编译产物与清单文件复制到服务端 Plugify 环境目录：
   - **Windows 平台**：
     - `game/csgo/addons/plugify/envs/funrounds/bin/funrounds_plugin.dll`
     - `game/csgo/addons/plugify/envs/funrounds/funrounds.pplugin`
   - **Linux 平台**：
     - `game/csgo/addons/plugify/envs/funrounds/bin/funrounds_plugin.so`
     - `game/csgo/addons/plugify/envs/funrounds/funrounds.pplugin`

2. **配置插件**：
   将 [config/funrounds.example.toml](config/funrounds.example.toml) 复制到服务端配置目录：
   - 目标路径：`game/csgo/addons/plugify/configs/funrounds.toml`
   - 根据需求调整模式参数与权重。

### 配置说明

配置文件采用严格 TOML 语法解析。基础配置示例如下：

```toml
# 顶层全局设置
diagnostics = false # 是否开启诊断日志（默认 false）

[[modes]]
id = "example"              # 模式唯一标识（仅允许字母、数字、'_' 或 '-'）
enabled = true              # 是否启用该模式并加入随机池
name = "鸟狙低重力"          # 游戏中显示的模式名称
description = "低重力狙击对抗"
weapons = ["weapon_ssg08", "weapon_glock"]

# 基础属性
health = 100                # 初始生命值 (1～10000)
armor = 100                 # 初始护甲值 (0～255)
gravity = 0.5               # 重力倍率 (0.0 < g <= 10.0，1.0 为正常)

# 可选机制开关
bunnyhop = true             # 是否开启自动连跳
no_spread_recoil = false    # 是否消除武器后坐力与散布
unlimited_reserve = true    # 是否开启无限备弹
lifesteal = false           # 是否开启造成伤害吸血

# 特殊 C4 规则（可选原生 native、鸡背 chicken 或可携带 carry）
# [modes.bomb]
# kind = "native"
# plant_time = 3.0
# explode_time = 45.0
# defuse_kit_time = 5.0
# defuse_time = 10.0
# bounce = false
```

### 指令列表

#### 管理员与服务端控制台指令（Console / RCON）

| 控制台指令 | 参数 | 说明 |
| :--- | :--- | :--- |
| `fr_status` | 无 | 查看当前回合状态、运行阶段、底层能力就绪状态及故障排查信息。 |
| `fr_list` | 无 | 列出当前配置池中所有已启用的特殊模式列表。 |
| `fr_next` | `<id \| random>` | 指定下一回合的模式 ID，或恢复随机抽取 (`random`)。 |
| `fr_reload` | 无 | 校验并重载配置文件，重载成功于下一回合生效。 |
| `fr_enable` | `<0 \| 1>` | 插件总启停开关：`1` 为启用，`0` 为停用。 |

#### 玩家控制台与聊天指令

| 控制台指令 | 聊天快捷指令 | 说明 |
| :--- | :--- | :--- |
| `fr_info` | `!frinfo` | 在聊天栏中查看当前回合模式名称、编号与玩法说明。 |
| `fr_help` | `!frhelp` | 查看插件帮助信息及玩家可用指令。 |
| `fr_knock` | `!knock [0\|1]` | 开启、关闭或切换自身开枪自推反冲（无参数为切换）。 |

### 源码编译

编译前请确保安装了 Rust `1.99.1`（可通过仓库内的 `rust-toolchain.toml` 自动匹配）。

```bash
# 代码格式化检查
cargo fmt --all -- --check

# Clippy 严格静态代码检查
cargo clippy --workspace --all-targets --locked -- -D warnings

# 运行自动化单元测试 (包含 119+ 完整测试用例)
cargo test --workspace --locked

# 编译 Release 发布版本
cargo build --release --workspace --locked
```

编译产物位于 `target/release/`：
- Windows: `funrounds_plugin.dll`
- Linux: `libfunrounds_plugin.so`（部署时需重命名为 `funrounds_plugin.so`）

---

### 开源协议与作者 / License & Author

- **Author**: Mai Ooizumi ([@Mai-Ooizumi](https://github.com/Mai-Ooizumi))
- **License**: GNU Affero General Public License v3.0 or later ([AGPL-3.0-or-later](LICENSE))
