// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use funrounds_core::{TextBundle, TextCatalog};
use std::collections::BTreeMap;
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

/// Preserve the previous round's label in available languages so a default-language
/// reload does not leave the global previous-round team name in the old language.
#[derive(Clone, Debug)]
pub(crate) struct ModeNames {
    localized: BTreeMap<String, String>,
    historical: String,
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
        let localized = texts
            .external_languages()
            .chain(["zh-CN", "en", texts.default_language()])
            .map(|language| (language.to_owned(), for_default(language)))
            .collect();
        Ok(Self { localized, historical })
    }
    pub(crate) fn for_default(&self, language: &str) -> &str {
        self.localized.get(language).map_or(&self.historical, String::as_str)
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
    let paths = match catalog_paths(directory) {
        Ok(paths) => paths,
        Err(error) if strict => return Err(error),
        Err(error) => {
            reports.push(error);
            Vec::new()
        }
    };
    for path in paths {
        let language = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        match std::fs::read_to_string(&path) {
            Ok(source) => candidates.push((
                language.clone(),
                TextCatalog::parse_for_language(&source, &language).map_err(|error| format!("{}: {error}", path.display())),
            )),
            Err(error) => candidates.push((language, Err(format!("{}: {error}", path.display())))),
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
    let mut languages = texts.external_languages().collect::<Vec<_>>();
    languages.sort_unstable();
    for language in languages {
        let missing = texts.missing_keys(language);
        if !missing.is_empty() {
            reports.push(format!("{language}: {} missing keys use fallback", missing.len()));
        }
    }
    if !reports.is_empty() {
        crate::s2sdk::PrintToServer(&plugify::Str::from_str(&format!("[FunRounds] text catalogs: {}", reports.join("; "))));
    }
    Ok(texts)
}

fn catalog_paths(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("{}: {error}", directory.display())),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry.map_err(|error| format!("{}: {error}", directory.display()))?.path();
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
            // Keep unreadable entries as candidates: startup rejects that file, not every valid locale.
            && std::fs::metadata(&path).map_or(true, |metadata| metadata.is_file())
        {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
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
