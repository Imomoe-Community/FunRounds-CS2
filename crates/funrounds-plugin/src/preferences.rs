// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Knock {
    Off,
    #[default]
    Airborne,
    Always,
}

impl Knock {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Airborne,
            Self::Airborne => Self::Always,
            Self::Always => Self::Off,
        }
    }

    pub fn permits(self, flags: i32) -> bool {
        self == Self::Always || (self == Self::Airborne && flags & 1 == 0)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "关闭",
            Self::Airborne => "仅空中",
            Self::Always => "地面及空中",
        }
    }
}

pub fn authenticated_id(slot: i32) -> Option<u64> {
    use crate::s2sdk;
    if slot < 0 || slot >= s2sdk::GetMaxClients() || !s2sdk::IsClientInGame(slot) || s2sdk::IsFakeClient(slot) || !s2sdk::IsClientAuthorized(slot) {
        return None;
    }
    let id = s2sdk::GetClientSteamID64(slot);
    (id != 0).then_some(id)
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    knock: BTreeMap<String, Knock>,
}

struct Session {
    id: Option<u64>,
    value: Knock,
    changed: bool,
}

pub struct Preferences {
    path: Option<PathBuf>,
    saved: Result<Saved, String>,
    sessions: HashMap<i32, Session>,
}

impl Preferences {
    pub fn new() -> Self {
        let directory = plugify::get_configs_dir().to_string();
        Self::at((!directory.is_empty()).then(|| Path::new(&directory).join("fr_players.toml")))
    }

    fn at(path: Option<PathBuf>) -> Self {
        let saved = path.as_deref().ok_or_else(|| "插件数据目录不可用".to_owned()).and_then(read);
        Self {
            path,
            saved,
            sessions: HashMap::new(),
        }
    }

    pub fn load_error(&self) -> Option<&str> {
        self.saved.as_ref().err().map(String::as_str)
    }

    pub fn value(&mut self, slot: i32, id: Option<u64>) -> Knock {
        let id = id.filter(|id| *id != 0);
        let saved = id
            .and_then(|id| self.saved.as_ref().ok()?.knock.get(&id.to_string()).copied())
            .unwrap_or_default();
        let session = self.sessions.entry(slot).or_insert(Session {
            id,
            value: saved,
            changed: false,
        });
        if session.id != id {
            // Authentication may finish after connect; a session-only choice stays in this connection.
            if session.id.is_some() || !session.changed {
                session.value = saved;
                session.changed = false;
            }
            session.id = id;
        }
        session.value
    }

    pub fn set(&mut self, slot: i32, id: Option<u64>, requested: Option<Knock>) -> String {
        let current = self.value(slot, id);
        let value = requested.unwrap_or_else(|| current.next());
        let session = self.sessions.get_mut(&slot).expect("session initialized");
        session.value = value;
        session.changed = true;
        let Some(id) = id.filter(|id| *id != 0) else {
            return format!("开枪自推：{}（仅本次连接；Bot或尚未认证）", value.label());
        };
        let result = self.path.as_deref().ok_or_else(|| "插件数据目录不可用".to_owned()).and_then(|path| {
            let mut saved = read(path)?;
            saved.knock.insert(id.to_string(), value);
            write(path, &saved)?;
            Ok(saved)
        });
        match result {
            Ok(saved) => {
                self.saved = Ok(saved);
                format!("开枪自推：{}（已保存）", value.label())
            }
            Err(error) => format!("开枪自推：{}（本次连接已生效，保存失败：{error}；旧文件未覆盖）", value.label()),
        }
    }

    pub fn forget(&mut self, slot: i32) {
        self.sessions.remove(&slot);
    }

    pub fn clear_sessions(&mut self) {
        self.sessions.clear();
    }
}

fn read(path: &Path) -> Result<Saved, String> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Saved::default()),
        Err(_) => return Err("无法读取偏好文件".into()),
    };
    // Parser diagnostics can contain account keys; never send them to chat or server logs.
    let saved: Saved = toml::from_str(&source).map_err(|_| "偏好文件损坏，请管理员修复")?;
    if saved
        .knock
        .keys()
        .any(|key| key.parse::<u64>().ok().is_none_or(|id| id == 0 || id.to_string() != *key))
    {
        return Err("偏好文件包含无效账号键，请管理员修复".into());
    }
    Ok(saved)
}

fn write(path: &Path, saved: &Saved) -> Result<(), String> {
    let source = toml::to_string(saved).map_err(|_| "无法编码偏好文件")?;
    let parent = path.parent().ok_or("偏好目录不可用")?;
    fs::create_dir_all(parent).map_err(|_| "无法创建偏好目录")?;
    let temporary = path.with_extension("toml.tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| "无法创建偏好临时文件")?;
    let result = file.write_all(source.as_bytes()).and_then(|()| file.sync_all());
    drop(file);
    let result = result.and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
        return Err("无法写入偏好文件".into());
    }
    Ok(())
}
