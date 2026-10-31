<div align="center">

# FunRounds-CS2

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/Version-0.3.0--alpha-informational.svg)](Cargo.toml)
[![Rust Version](https://img.shields.io/badge/Rust-1.99.1+-orange.svg)](rust-toolchain.toml)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20x64-lightgrey.svg)]()
[![Framework](https://img.shields.io/badge/Framework-Plugify%20%2B%20S2SDK%203.0.6-success.svg)](https://github.com/untrustedmodders/plugify)

**高性能、高安全性的 Counter-Strike 2 专用服务端特殊娱乐回合插件**<br>
<sub><sup>A high-performance, safe, and modular special rounds plugin for Counter-Strike 2 dedicated servers.</sub></sup>

[English](README_EN.md) • [简体中文](README_ZH.md)

</div>

---

### 项目简介 / Overview

**FunRounds-CS2** 是基于 Rust 与 C++20 原生扩展开发、面向 Counter-Strike 2 专用服务端的特殊娱乐回合（Fun Rounds）插件，运行于 [Plugify](https://github.com/untrustedmodders/plugify) 插件框架和 [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk) 之上。
<sub><sup>**FunRounds-CS2** is a modern special rounds plugin for Counter-Strike 2 dedicated servers developed in Rust and native C++20, running atop [Plugify](https://github.com/untrustedmodders/plugify) and [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk).</sub></sup>

在每个回合开始时（或由管理员实时指定），插件无缝应用多维度的玩法突变：修改玩家属性、物理参数、动态配装、后坐与视角机制、特殊战斗判定、小鸡生态及 C4 玩法，同时保证严格的崩溃隔离与无残留状态恢复。
<sub><sup>At the start of each round (or on administrative demand), it seamlessly applies multi-dimensional gameplay mutators: altering player attributes, physics, dynamic loadouts, recoil/camera mechanics, combat rules, chicken ecology, and bomb systems—with zero-leak state restoration and crash isolation.</sub></sup>

---

### 核心特性 / Key Features

| 维度 / <sub><sup>Category</sub></sup> | 玩法与机制说明 / <sub><sup>Description</sub></sup> |
| :--- | :--- |
| **基础属性与物理**<br><sub><sup>Attributes & Physics</sub></sup> | 生命值（1～10000 或随机区间）、护甲头盔、重力倍率、移速倍率、地面摩擦力、自动连跳（Bunnyhop）、地面加速度与反向倒车行走（`ground_reverse`）、全局时间流速（`time_scale`）及周期性变重力/变移速。<br><sub><sup>Health (fixed or random range), armor, gravity, speed multiplier, friction, auto-bunnyhopping, ground acceleration, ground reverse walking, time scale, and periodic oscillation.</sub></sup> |
| **视觉与相机视角**<br><sub><sup>Visuals & Camera</sub></sup> | 第三人称视角（`third_person`）、幽灵按行动隐身（`ghost`，支持呼吸心跳音效与距离感知）、夜视仪（`fr_nvg`）、准星/雷达隐藏（`hide_hud`）、模型 RGB 着色、半透明外观与开镜限制。<br><sub><sup>Third person camera, stealth ghost mode with breathing audio cues, night vision, HUD hiding, model RGB tint, translucency, and scope restrictions.</sub></sup> |
| **射击与自推反冲**<br><sub><sup>Recoil & Knockback</sub></sup> | 开火自身反冲自推（`fr_knock` 支持地面与空中切换）、后坐力与散布消除、开枪镜头震动冲击与衰减（`recoil_view` / `view_punch_decay`）、快速切枪（`fast_switch`）及射击引爆掉落手雷。<br><sub><sup>Shooting self-push knockback (ground/air modes), recoil & spread suppression, view-punch intensity/decay, fast switch, and shooting dropped grenades.</sub></sup> |
| **动态配装与弹药**<br><sub><sup>Loadouts & Ammo</sub></sup> | 阵营武器清理与定向配发、4 种弹药模式（原生 / 无限备弹 / 无限弹匣 / 单发换填）、电击枪充能、手雷自动补充（HE/闪光/烟雾/诱饵/燃烧弹）、击杀升级换枪（`kill_loadout`）与随机抽枪。<br><sub><sup>Team-specific weapon clearing/giving, 4 ammo modes (inherit/reserve/clip/single-shot), taser recharge, grenade refills, kill-progression, and random loadouts.</sub></sup> |
| **特殊战斗判定**<br><sub><sup>Combat Modifiers</sub></sup> | 造成伤害吸血、反弹伤害（`reflect_damage`）、按当前生命动态缩放伤害/移速/衰减、能量吸收护盾、开枪自伤（未命中扣血）、诱饵弹躲避球碰撞杀伤（`dodgeball`）、右键飞刀投掷弹射与命中持续燃烧（`afterburn`）。<br><sub><sup>Vampiric lifesteal, reflect damage, HP-scaled damage/speed/decay, energy shield, miss penalty self-damage, decoy dodgeball, throwing knives, and afterburn.</sub></sup> |
| **C4 炸弹与自爆**<br><sub><sup>C4 Objectives & Suicide</sub></sup> | 3 种炸弹模式（原生 C4 / 鸡背 C4 / 便携可携带 C4）、自定义安放/拆除/爆炸耗时、炸弹弹跳、4 色引线拆包剪线小游戏（`fr_wire`）、现金购买 C4（`fr_c4`）及引爆炸弹自爆（`fr_suicide`）。<br><sub><sup>3 bomb types (native, chicken, carry), custom timers, bomb bouncing, 4-color wire cutting minigame, C4 purchasing, and suicide detonation.</sub></sup> |
| **小鸡生态与回血**<br><sub><sup>Chicken & Healthshots</sub></sup> | 普通小鸡比例刷新生态、互动吃鸡回血与未消化自爆风险机制、小鸡连锁爆炸；自定义医疗针回血比例与上限控制（`healthshot`，支持 `fr_healthshot` 购买）。<br><sub><sup>Chicken population scaling, eating chickens with digestion risk & chain explosions; custom healthshot healing ratios with cash purchase.</sub></sup> |
| **经济与多语言**<br><sub><sup>Economy & i18n</sub></sup> | 任意位置购买、阵营购买权限、回合保底现金与被动收入；内置完整中英多语言本地化（`zh-CN` / `en`），优先跟随客户端语言。<br><sub><sup>Buy anywhere, team purchase access, round money floor, passive income, and built-in bilingual i18n matching client preferences.</sub></sup> |

<details>
<summary><b>点击展开架构与可靠性设计 / Click to expand Architecture & Reliability</b></summary>

- **三层模块化架构**：
  - `funrounds-core`：纯 Rust 编写、独立于引擎的核心库。负责玩法规则、状态机流转、`[defaults]` 模板继承、数学判定及严格校验的 TOML 配置解析。
  - `funrounds-plugin`：Plugify Rust 原生适配插件。在游戏主线程响应 S2SDK 实体事件，管理实体生命周期与多语言反馈。
  - `funrounds-native`：C++20 原生伴侣模块。提供底层伤害桥接、投射物 Touch、实体碰撞拦截、小鸡互动、幽灵雷达遮蔽及 Gamedata 偏移管理。
- **严格的安全隔离**：
  - **FFI Panic 全边界捕获**：所有跨 C ABI 边界的回调均由 `catch_unwind` 包裹，防止单次回调异常波及专用服务端进程。
  - **事务性状态回滚**：严格记录代次（Generation）与实体快照，在回合结束或插件停用时仅还原受管属性与 ConVar，绝不残留脏状态。
  - **能力动态隔离**：根据服务端运行环境的 SDK 钩子支持情况，自动隔离缺失底层能力的模式，其余模式不受影响正常运行。
</details>

---

### 项目目录结构 / Project Layout

```text
├── bindings/             # Plugify S2SDK 接口清单 (123 项接口定义) / Provider manifest
├── config/               # 示例配置文件及多语言包 / Configurations & i18n files
│   ├── funrounds.example.toml # 示例配置 / Reference configuration
│   └── i18n/             # 本地化翻译文件 (zh-CN.toml, en.toml) / Localization catalogs
├── crates/
│   ├── funrounds-core/   # 独立于引擎的玩法核心、规则校验与配置解析 / Core rules & config parser
│   └── funrounds-plugin/ # 游戏线程 Plugify 插件运行时 / Plugify game runtime plugin
├── native/
│   └── funrounds-native/ # 原生 C++ 伴侣扩展 (底层挂钩、碰撞拦截、Gamedata) / Native C++ companion
├── packaging/            # Rust 插件包清单 (funrounds.pplugin) / Plugify package manifest
├── scripts/              # 原生构建与 CI 自动化脚本 / Build & CI automation scripts
└── rust-toolchain.toml   # 固定编译器工具链版本 (Rust 1.99.1) / Pinned Rust toolchain
```

---

### 安装与部署 / Installation

#### 前置要求 / Prerequisites

- Counter-Strike 2 专用服务端（Windows x64 或 Linux x64）<br><sub><sup>Counter-Strike 2 Dedicated Server (Windows x64 or Linux x64)</sub></sup>
- 已安装并运行 [Plugify](https://github.com/untrustedmodders/plugify) 插件框架<br><sub><sup>[Plugify](https://github.com/untrustedmodders/plugify) framework installed and active</sub></sup>
- 已配置并加载 [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk) 插件（版本 `3.0.6`）<br><sub><sup>[S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk) plugin (version `3.0.6`) active in Plugify</sub></sup>

#### 目录部署 / Deployment Structure

将编译产物与配置文件复制到服务端 `game/csgo/addons/plugify/` 对应路径：
<sub><sup>Copy the build artifacts and configurations into `game/csgo/addons/plugify/`:</sub></sup>

```text
game/csgo/addons/plugify/
├── configs/
│   ├── funrounds.toml                              # 从 config/funrounds.example.toml 复制
│   └── funrounds/
│       └── i18n/                                   # 复制 config/i18n/ 下的翻译文件
│           ├── zh-CN.toml
│           └── en.toml
└── envs/
    ├── funrounds/                                  # Rust 主插件 / Main Rust Plugin
    │   ├── bin/
    │   │   └── funrounds_plugin.dll (或 .so)       # 动态库 / Plugin binary
    │   └── funrounds.pplugin                       # 插件清单 / Manifest
    └── funrounds_native/                           # 原生 C++ 伴侣扩展 / Native C++ Companion
        ├── bin/
        │   └── funrounds-native.dll (或 lib*.so)   # 原生动态库 / Native binary
        ├── funrounds-native.pplugin                # 原生插件清单 / Native manifest
        └── gamedata/
            └── funrounds-native.jsonc              # Gamedata 特征与偏移 / Gamedata offsets
```

---

### 指令列表 / Commands Reference

#### 管理员与控制台指令（Console / RCON） / Server & Administrator Commands

| 控制台指令 <br><sub><sup>Command</sub></sup> | 参数 <br><sub><sup>Arguments</sub></sup> | 说明 <br><sub><sup>Description</sub></sup> |
| :--- | :--- | :--- |
| `fr_status` | 无 <br><sub><sup>None</sub></sup> | 查看当前回合状态、运行阶段、底层能力就绪状态及故障排查信息。<br><sub><sup>Displays current mode, phase, capability readiness, and fault diagnoses.</sub></sup> |
| `fr_list` | 无 <br><sub><sup>None</sub></sup> | 列出配置池中所有已启用的特殊模式列表。<br><sub><sup>Lists all loaded and enabled round modes in the pool.</sub></sup> |
| `fr_next` | `<id \| random>` | 指定下一回合的模式 ID，或恢复随机抽取 (`random`)。<br><sub><sup>Queues a specific mode ID or random selection for the next round.</sub></sup> |
| `fr_reload` | 无 <br><sub><sup>None</sub></sup> | 校验并重载配置文件与多语言文本，重载成功于下一回合生效。<br><sub><sup>Validates and schedules configuration and i18n reload for the next round.</sub></sup> |
| `fr_enable` | `<0 \| 1>` | 插件总启停开关：`1` 为启用，`0` 为停用。<br><sub><sup>Master switch: disables (`0`) or enables (`1`) the plugin.</sub></sup> |

#### 玩家控制台与聊天快捷指令 / Player Commands (Console & Chat)

| 控制台指令 <br><sub><sup>Console</sub></sup> | 聊天指令 <br><sub><sup>Chat Alias</sub></sup> | 参数 <br><sub><sup>Arguments</sub></sup> | 说明 <br><sub><sup>Description</sub></sup> |
| :--- | :--- | :--- | :--- |
| `fr_info` | `!info` / `!frinfo` | 无 <br><sub><sup>None</sub></sup> | 在聊天栏中查看当前回合模式名称、编号与玩法说明。<br><sub><sup>Displays current round mode name, ID, and description in chat.</sub></sup> |
| `fr_help` | `!help` / `!frhelp` | 无 <br><sub><sup>None</sub></sup> | 查看插件帮助信息及玩家可用指令列表。<br><sub><sup>Displays available player commands and usage instructions.</sub></sup> |
| `fr_knock` | `!knock` | `[0\|1\|2]` | 设置或切换开火自推反冲（`0` 关闭，`1` 地空全开，`2` 仅空中 [默认]，无参依次切换）。<br><sub><sup>Configures recoil self-push (`0`: off, `1`: ground & air, `2`: airborne only [default]; cycle without args).</sub></sup> |
| `fr_c4` | `!c4` | 无 <br><sub><sup>None</sub></sup> | 进攻方在模式允许时按配置价格购买额外 C4。<br><sub><sup>T players purchase extra C4 at the configured price when allowed.</sub></sup> |
| `fr_suicide` | `!suicide` | 无 <br><sub><sup>None</sub></sup> | 允许自爆时使用所持或购买的 C4 启动自爆。<br><sub><sup>Triggers suicide explosion with held or purchased C4 when allowed.</sub></sup> |
| `fr_chicken` | `!chicken` | 无 <br><sub><sup>None</sub></sup> | 按配置价格在当前位置购买一只小鸡。<br><sub><sup>Purchases an ordinary chicken at the configured price.</sub></sup> |
| `fr_healthshot` | `!healthshot` | 无 <br><sub><sup>None</sub></sup> | 按配置价格购买一支医疗针。<br><sub><sup>Purchases a healthshot at the configured price.</sub></sup> |
| `fr_nvg` | `!nvg` | 无 <br><sub><sup>None</sub></sup> | 开启或关闭夜视仪。<br><sub><sup>Toggles night vision.</sub></sup> |
| `fr_wire` | `!wire` | `<color>` | 选择安包引线或拆包剪线颜色（`blue` / `yellow` / `red` / `green`）。<br><sub><sup>Selects planting or quick-defusing wire color (blue, yellow, red, green).</sub></sup> |

---

### 配置说明 / Configuration

配置文件采用严格 TOML 语法解析。支持 `[defaults]` 模板为所有模式提供基础属性继承：
<sub><sup>Configuration uses strict TOML syntax with a global `[defaults]` template for field inheritance:</sub></sup>

```toml
# 全局设置 / Global settings
diagnostics = false # 诊断日志开关 / Diagnostic logging

# 多语言配置 / i18n
[i18n]
default_language = "zh-CN" # 服务端缺省语言（"zh-CN" 或 "en"）/ Default language

# 全局默认模板（模式内未设置的字段将继承此处） / Global defaults
[defaults]
gravity = 1.0
bunnyhop = false

[[modes]]
id = "example"              # 模式唯一标识 / Unique mode identifier
enabled = true              # 是否启用该模式 / Whether mode enters the pool
name_key = "mode.example.name" # 多语言名称键 / Localized name key
description_key = "mode.example.description" # 多语言说明键 / Localized description key
weapons = ["weapon_famas", "weapon_p250"]

# 阵营装备配发与清理 / Team equipment distribution
clear_weapons = "all"       # 配发前清理武器的队伍 (none / all / terrorists / counter_terrorists)
give_weapons = "all"        # 配发候选武器的队伍
give_knife = "all"          # 配发刀具的队伍

# 基础属性与移动 / Attributes & movement
health = 120                # 初始生命值 (1～10000 或随机范围 { min = 100, max = 150 })
armor = 50                  # 初始护甲值 (0～255 或随机范围 { min = 0, max = 100 })
gravity = 1.0               # 重力倍率 ((0, 10]，1.0 为正常重力)
speed = 1.0                 # 移速倍率 ((0, 10])
self_push = true            # 开启开火自推反冲 / Recoil self-push

# 射击与视角控制 / Shooting & view dynamics
ammo = "reserve"            # 弹药规则 (inherit / reserve / clip / single_shot)
headshots = "normal"        # 爆头规则 (normal / only / disabled)
fast_switch = true          # 快速切枪开关 / Fast switch

# 特殊玩法机制选开 / Optional mutators
# lifesteal = true          # 吸血恢复 / Vampiric lifesteal
# suicide = true            # 允许自爆玩法 / Suicide blast
# third_person = { distance = 64.0, follow_eyes = true, return_strength = 2.0 } # 第三人称
# bomb = { kind = "native", plant_time = 3.0, explode_time = 45.0, bounce = false } # 特殊炸弹
```

<details>
<summary><b>查看完整配置选项注释 / View Full Configuration Reference</b></summary>

请直接查阅 [config/funrounds.example.toml](config/funrounds.example.toml) 获取完整的字段说明，涵盖小鸡生态、伤害缩放曲线、幽灵隐身、特殊 C4、购买与经济等全部选项。
<sub><sup>Refer to [config/funrounds.example.toml](config/funrounds.example.toml) for full documentation of chicken ecology, health curves, ghost stealth, custom bombs, and economy rules.</sub></sup>
</details>

---

### 源码编译 / Building from Source

#### 1. 编译 Rust 插件 / Build Rust Plugin

工具链要求：Rust `1.99.1`（由 `rust-toolchain.toml` 自动匹配）：
<sub><sup>Requires Rust `1.99.1` (automatically handled by `rust-toolchain.toml`):</sub></sup>

```bash
# 代码格式化与 Clippy 严格静态检查
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# 执行全量自动化单元测试
cargo test --workspace --locked

# 编译 Release 发布产物
cargo build --release --workspace --locked
```

#### 2. 编译原生 C++ 伴侣扩展 / Build Native C++ Companion

要求 Python 3.10+、CMake 3.25+ 以及对应平台的 C++20 编译器（MSVC 2022+ 或 GCC 11+）：
<sub><sup>Requires Python 3.10+, CMake 3.25+, and a C++20 compiler (MSVC 2022+ or GCC 11+):</sub></sup>

```bash
# 构建原生扩展并输出至 dist/native
python scripts/build-native.py --output-dir dist/native
```

---

### 开源协议与作者 / License & Author

- **作者 / Author**: Mai Ooizumi ([@Mai-Ooizumi](https://github.com/Mai-Ooizumi))
- **开源协议 / License**: GNU Affero General Public License v3.0 or later ([AGPL-3.0-or-later](LICENSE))
