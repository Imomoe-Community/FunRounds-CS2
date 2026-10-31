<div align="center">

# FunRounds-CS2

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/Version-0.3.0--alpha-informational.svg)](Cargo.toml)
[![Rust Version](https://img.shields.io/badge/Rust-1.99.1+-orange.svg)](rust-toolchain.toml)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20x64-lightgrey.svg)]()
[![Framework](https://img.shields.io/badge/Framework-Plugify%20%2B%20S2SDK%203.0.6-success.svg)](https://github.com/untrustedmodders/plugify)

**高性能、高安全性的 Counter-Strike 2 专用服务端特殊娱乐回合插件**

[English](README_EN.md) • [简体中文](README_ZH.md)

</div>

---

### 项目简介

**FunRounds-CS2** 是基于 Rust 与 C++20 原生扩展开发、面向 Counter-Strike 2 专用服务端的特殊娱乐回合（Fun Rounds）插件。本项目构建于 [Plugify](https://github.com/untrustedmodders/plugify) 插件框架和 [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk) 扩展之上。

插件在每个回合开始时（或由管理员实时指定）无缝应用多维度的玩法突变：修改玩家属性、物理参数、动态配装、后坐与视角机制、特殊战斗规则、小鸡生态及 C4 玩法，同时保证严格的崩溃隔离与无残留状态恢复。

---

### 核心特性

| 玩法维度 | 特性机制概要 |
| :--- | :--- |
| **基础属性与物理** | 生命值（1～10000 或区间随机）、护甲头盔、重力倍率、移速倍率、地面摩擦力、自动连跳（Bunnyhop）、地面加速度与反向倒车行走（`ground_reverse`）、全局时间流速（`time_scale`）及周期性变重力/变移速。 |
| **视觉与相机视角** | 第三人称视角（`third_person`）、幽灵按行动隐身（`ghost`，支持呼吸心跳音效与距离感知）、夜视仪（`fr_nvg`）、准星/雷达隐藏（`hide_hud`）、模型 RGB 着色、半透明外观与开镜限制。 |
| **射击与自推反冲** | 开火自身反冲自推（`fr_knock` 支持地面与空中切换）、后坐力与散布消除、开枪镜头震动冲击与衰减（`recoil_view` / `view_punch_decay`）、快速切枪（`fast_switch`）及射击引爆掉落手雷。 |
| **动态配装与弹药** | 阵营武器清理与定向配发、4 种弹药模式（原生 / 无限备弹 / 无限弹匣 / 单发换装）、电击枪充能、手雷自动补充（HE/闪光/烟雾/诱饵/燃烧弹）、击杀升级换枪（`kill_loadout`）与随机抽枪。 |
| **特殊战斗判定** | 造成伤害吸血、反弹伤害（`reflect_damage`）、按当前生命动态缩放伤害/移速/衰减、能量吸收护盾、开枪自伤（未命中扣血）、诱饵弹躲避球碰撞杀伤（`dodgeball`）、右键飞刀投掷弹射与命中持续燃烧（`afterburn`）。 |
| **C4 炸弹与自爆** | 3 种炸弹模式（原生 C4 / 鸡背 C4 / 便携可携带 C4）、自定义安放/拆除/爆炸耗时、炸弹弹跳、4 色引线拆包剪线小游戏（`fr_wire`）、现金购买 C4（`fr_c4`）及引爆炸弹自爆（`fr_suicide`）。 |
| **小鸡生态与回血** | 普通小鸡比例刷新生态、互动吃鸡回血与未消化自爆风险机制、小鸡连锁爆炸；自定义医疗针回血比例与上限控制（`healthshot`，支持 `fr_healthshot` 购买）。 |
| **经济与多语言** | 任意位置购买、阵营购买权限、回合保底现金与被动收入；内置完整中英多语言本地化（`zh-CN` / `en`），优先跟随客户端语言。 |

<details>
<summary><b>点击展开架构与可靠性设计</b></summary>

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

### 项目目录结构

```text
├── bindings/             # Plugify S2SDK 接口清单 (123 项接口定义)
├── config/               # 示例配置文件及多语言包
│   ├── funrounds.example.toml # 示例配置
│   └── i18n/             # 本地化翻译文件 (zh-CN.toml, en.toml)
├── crates/
│   ├── funrounds-core/   # 独立于引擎的玩法核心、规则校验与配置解析
│   └── funrounds-plugin/ # 游戏线程 Plugify 插件运行时实现
├── native/
│   └── funrounds-native/ # 原生 C++ 伴侣扩展 (底层挂钩、碰撞拦截、Gamedata 偏移)
├── packaging/            # Rust 插件包清单 (funrounds.pplugin)
├── scripts/              # 原生构建与 CI 自动化脚本
└── rust-toolchain.toml   # 固定编译器工具链版本 (Rust 1.99.1)
```

---

### 安装与部署

#### 前置要求

- Counter-Strike 2 专用服务端（Windows x64 或 Linux x64）
- 服务端已安装配置 [Plugify](https://github.com/untrustedmodders/plugify) 插件框架
- 服务端已加载并运行 [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk) 插件（版本 `3.0.6`）

#### 部署步骤

将编译产物与配置文件复制到服务端 `game/csgo/addons/plugify/` 对应路径：

```text
game/csgo/addons/plugify/
├── configs/
│   ├── funrounds.toml                              # 从 config/funrounds.example.toml 复制
│   └── funrounds/
│       └── i18n/                                   # 复制 config/i18n/ 下的翻译文件
│           ├── zh-CN.toml
│           └── en.toml
└── envs/
    ├── funrounds/                                  # Rust 主插件
    │   ├── bin/
    │   │   └── funrounds_plugin.dll (或 .so)       # 动态库
    │   └── funrounds.pplugin                       # 插件清单
    └── funrounds_native/                           # 原生 C++ 伴侣扩展
        ├── bin/
        │   └── funrounds-native.dll (或 lib*.so)   # 原生动态库
        ├── funrounds-native.pplugin                # 原生插件清单
        └── gamedata/
            └── funrounds-native.jsonc              # Gamedata 特征与偏移文件
```

---

### 指令列表

#### 管理员与服务端控制台指令（Console / RCON）

| 控制台指令 | 参数 | 说明 |
| :--- | :--- | :--- |
| `fr_status` | 无 | 查看当前回合状态、运行阶段、底层能力就绪状态及故障排查信息。 |
| `fr_list` | 无 | 列出当前配置池中所有已启用的特殊模式列表。 |
| `fr_next` | `<id \| random>` | 指定下一回合的模式 ID，或恢复随机抽取 (`random`)。 |
| `fr_reload` | 无 | 校验并重载配置文件与多语言文本，重载成功于下一回合生效。 |
| `fr_enable` | `<0 \| 1>` | 插件总启停开关：`1` 为启用，`0` 为停用。 |

#### 玩家控制台与聊天指令

| 控制台指令 | 聊天快捷指令 | 参数 | 说明 |
| :--- | :--- | :--- | :--- |
| `fr_info` | `!info` / `!frinfo` | 无 | 在聊天栏中查看当前回合模式名称、编号与玩法说明。 |
| `fr_help` | `!help` / `!frhelp` | 无 | 查看插件帮助信息及玩家可用指令。 |
| `fr_knock` | `!knock` | `[0\|1\|2]` | 设置或切换开火自推反冲（`0` 关闭，`1` 地空全开，`2` 仅空中 [默认]，无参依次切换）。 |
| `fr_c4` | `!c4` | 无 | 进攻方在模式允许时按配置价格购买额外 C4。 |
| `fr_suicide` | `!suicide` | 无 | 允许自爆时使用所持或购买的 C4 启动自爆。 |
| `fr_chicken` | `!chicken` | 无 | 按配置价格在当前位置购买一只小鸡。 |
| `fr_healthshot` | `!healthshot` | 无 | 按配置价格购买一支医疗针。 |
| `fr_nvg` | `!nvg` | 无 | 开启或关闭夜视仪。 |
| `fr_wire` | `!wire` | `<color>` | 选择安包引线或拆包剪线颜色（`blue` / `yellow` / `red` / `green`）。 |

---

### 配置说明

配置文件采用严格 TOML 语法解析。支持 `[defaults]` 全局继承模板为所有模式提供默认参数：

```toml
# 顶层全局设置
diagnostics = false # 是否开启诊断日志（默认 false）

# 多语言配置
[i18n]
default_language = "zh-CN" # 服务端缺省语言（"zh-CN" 或 "en"）

# 全局默认模板（模式内未单独设置的字段将继承此处）
[defaults]
gravity = 1.0
bunnyhop = false

[[modes]]
id = "example"              # 模式唯一标识（仅允许字母、数字、'_' 或 '-'）
enabled = true              # 是否启用该模式并加入随机池
name_key = "mode.example.name" # 引用 i18n/*.toml 中的多语言文本键
description_key = "mode.example.description"
weapons = ["weapon_famas", "weapon_p250"]

# 阵营发放与清理
clear_weapons = "all"       # 发放前清理武器队伍 (none / all / terrorists / counter_terrorists)
give_weapons = "all"        # 发放候选武器队伍
give_knife = "all"          # 发放刀具队伍

# 基础属性与移动
health = 120                # 初始生命值 (1～10000，或区间随机 { min = 100, max = 150 })
armor = 50                  # 初始护甲值 (0～255，或区间随机 { min = 0, max = 100 })
gravity = 1.0               # 重力倍率 (0.0 < g <= 10.0，1.0 为正常重力)
speed = 1.0                 # 移速倍率
self_push = true            # 开启开火自推反冲

# 射击与视角控制
ammo = "reserve"            # 弹药模式 (inherit / reserve / clip / single_shot)
headshots = "normal"        # 爆头规则 (normal / only / disabled)
fast_switch = true          # 快速切枪开关

# 特殊玩法机制选开
# lifesteal = true          # 吸血恢复
# suicide = true            # 允许自爆玩法
# third_person = { distance = 64.0, follow_eyes = true, return_strength = 2.0 } # 第三人称视角
# bomb = { kind = "native", plant_time = 3.0, explode_time = 45.0, bounce = false } # 特殊炸弹
```

<details>
<summary><b>查看完整配置选项注释</b></summary>

请直接查阅 [config/funrounds.example.toml](config/funrounds.example.toml) 获取完整的字段说明，涵盖小鸡生态、伤害缩放曲线、幽灵隐身、特殊 C4、购买与经济等全部选项。
</details>

---

### 源码编译

#### 1. 编译 Rust 插件

编译前请确保安装了 Rust `1.99.1`（可通过仓库内的 `rust-toolchain.toml` 自动匹配）。

```bash
# 代码格式化检查与 Clippy 静态检查
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# 运行全量自动化单元测试
cargo test --workspace --locked

# 编译 Release 发布版本
cargo build --release --workspace --locked
```

#### 2. 编译原生 C++ 扩展

需要 Python 3.10+、CMake 3.25+ 以及对应平台的 C++20 编译器（MSVC 2022+ 或 GCC 11+）：

```bash
# 构建原生扩展并输出至 dist/native
python scripts/build-native.py --output-dir dist/native
```

---

### 开源协议与作者

- **作者**: Mai Ooizumi ([@Mai-Ooizumi](https://github.com/Mai-Ooizumi))
- **开源协议**: GNU Affero General Public License v3.0 or later ([AGPL-3.0-or-later](LICENSE))
