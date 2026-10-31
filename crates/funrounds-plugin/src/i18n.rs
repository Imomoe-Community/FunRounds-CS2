// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use funrounds_core::{TextBundle, TextCatalog};
use std::path::{Path, PathBuf};

/// Semantic feedback kept independent of the receiver's current language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Message {
    pub key: &'static str,
    pub values: Vec<(String, String)>,
}
impl Message {
    pub(crate) fn new(key: &'static str) -> Self {
        Self { key, values: Vec::new() }
    }
    pub(crate) fn arg(mut self, name: &str, value: impl ToString) -> Self {
        self.values.push((name.into(), value.to_string()));
        self
    }
    pub(crate) fn render(&self, texts: &TextBundle, language: &str) -> String {
        let args = self.values.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<Vec<_>>();
        texts
            .render(language, self.key, &args)
            .expect("validated feedback template and arguments")
    }
}

/// Preserve the previous round's label in both supported languages so a default-language
/// reload does not leave the global previous-round team name in the old language.
#[derive(Clone, Debug)]
pub(crate) struct ModeNames {
    zh_cn: String,
    en: String,
}
impl ModeNames {
    pub(crate) fn capture(mode: &funrounds_core::Mode, texts: &TextBundle) -> Result<Self, String> {
        let historical = mode.localized_name(texts, texts.default_language())?;
        let for_default = |language| {
            if !mode.name.is_empty() {
                return historical.clone();
            }
            // A retired custom key may never have had a translation for the new default.
            // Keep its original historical label instead of invalidating a later reload.
            texts
                .render_default(language, mode.name_key.as_deref().expect("validated mode label"), &[])
                .unwrap_or_else(|_| historical.clone())
        };
        Ok(Self {
            zh_cn: for_default("zh-CN"),
            en: for_default("en"),
        })
    }
    pub(crate) fn for_default(&self, language: &str) -> &str {
        if language == "en" { &self.en } else { &self.zh_cn }
    }
}

pub(crate) fn config_path(root: &Path) -> (PathBuf, Option<String>) {
    let old = root.join("funrounds.toml");
    let new = root.join("funrounds/funrounds.toml");
    let conflict = (old.exists() && new.exists()).then(|| {
        format!(
            "both configuration paths exist: {} and {}; using legacy {}",
            old.display(),
            new.display(),
            old.display()
        )
    });
    (if old.exists() { old } else { new }, conflict)
}

/// Startup rejects just the invalid catalog; reload rejects the whole candidate.
pub(crate) fn load(directory: &Path, default: &str, strict: bool, config: Option<&funrounds_core::Config>) -> Result<TextBundle, String> {
    let mut candidates = Vec::new();
    let mut reports = Vec::new();
    for language in ["zh-CN", "en"] {
        let path = directory.join(format!("{language}.toml"));
        match std::fs::read_to_string(&path) {
            Ok(source) => candidates.push((
                language.to_owned(),
                TextCatalog::parse_for_language(&source, language).map_err(|error| format!("{}: {error}", path.display())),
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => reports.push(format!("{} missing; using fallback", path.display())),
            Err(error) => candidates.push((language.to_owned(), Err(format!("{}: {error}", path.display())))),
        }
    }
    let texts = if strict {
        let catalogs = candidates
            .into_iter()
            .map(|(language, catalog)| catalog.map(|catalog| (language, catalog)))
            .collect::<Result<Vec<_>, _>>()?;
        TextBundle::load(default, catalogs)?
    } else {
        let (texts, rejected) = if let Some(config) = config {
            TextBundle::load_startup_for_config(default, candidates, config)?
        } else {
            TextBundle::load_startup(default, candidates)?
        };
        reports.extend(rejected);
        texts
    };
    for language in ["zh-CN", "en"] {
        let missing = texts.missing_keys(language);
        if !missing.is_empty() {
            reports.push(format!("{language}: {} missing keys use fallback: {}", missing.len(), missing.join(",")));
        }
    }
    if !reports.is_empty() {
        crate::s2sdk::PrintToServer(&plugify::Str::from_str(&format!("[FunRounds] text catalogs: {}", reports.join("; "))));
    }
    Ok(texts)
}

/// Unlike status replies, console text keeps whitespace and the original line layout.
pub(crate) fn console_parts(mut message: &str) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        if message.is_empty() {
            return None;
        }
        let end = message.floor_char_boundary(1024.min(message.len()));
        let (part, tail) = message.split_at(end);
        message = tail;
        Some(part)
    })
}

pub(crate) fn language(slot: i32, texts: &TextBundle) -> String {
    if slot >= 0 && unsafe { crate::s2sdk::__s2sdk_GetClientLanguageCode }.is_some() {
        return crate::s2sdk::GetClientLanguageCode(slot).to_string();
    }
    texts.default_language().to_owned()
}

/// Chat carries a leading colour over continuation chunks. Console uses its larger channel budget.
pub(crate) fn chat_parts(message: &str) -> Vec<String> {
    let mut result = Vec::new();
    for line in message.lines() {
        let mut rest = line;
        let mut color = None;
        while !rest.is_empty() {
            let prefix = color.map_or_else(String::new, |c: char| format!(" {c}"));
            let end = rest.floor_char_boundary((190 - prefix.len()).min(rest.len()));
            let (part, tail) = rest.split_at(end);
            result.push(format!("{prefix}{part}"));
            color = part.chars().rev().find(|c| matches!(*c as u32, 1..=16)).or(color);
            rest = tail;
        }
    }
    result
}

pub(crate) fn send_chat(slot: i32, message: &str) {
    for part in chat_parts(message) {
        crate::s2sdk::PrintToChat(slot, &plugify::Str::from_str(&part));
    }
}
