// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{Config, Mode};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};

const ZH_CN: &str = include_str!("../../../config/i18n/zh-CN.toml");
const EN: &str = include_str!("../../../config/i18n/en.toml");

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogInput {
    messages: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextCatalog {
    messages: BTreeMap<String, String>,
}

impl TextCatalog {
    pub fn parse(source: &str) -> Result<Self, String> {
        let input: CatalogInput = toml::from_str(source).map_err(|error| error.to_string())?;
        if input.messages.is_empty() {
            return Err("text catalog must contain at least one message".into());
        }
        for (key, text) in &input.messages {
            validate_key(key)?;
            placeholders(text).map_err(|error| format!("{key}: {error}"))?;
            if text.contains('\0') {
                return Err(format!("{key}: text contains a NUL character"));
            }
        }
        Ok(Self { messages: input.messages })
    }
    fn get(&self, key: &str) -> Option<&str> {
        self.messages.get(key).map(String::as_str)
    }
    pub fn parse_for_language(source: &str, language: &str) -> Result<Self, String> {
        let catalog = Self::parse(source)?;
        catalog.validate_language(language)?;
        Ok(catalog)
    }
    fn validate_language(&self, language: &str) -> Result<(), String> {
        validate_language(language)?;
        let embedded = TextCatalog::parse(match language {
            "zh-CN" => ZH_CN,
            "en" => EN,
            _ => unreachable!(),
        })?;
        validate_shared_placeholders(self, &embedded, language, "embedded")
    }
    fn validate_compatible_with(&self, language: &str, other: &Self, other_language: &str) -> Result<(), String> {
        validate_language(language)?;
        validate_language(other_language)?;
        validate_shared_placeholders(self, other, language, other_language)
    }
    fn keys(&self) -> impl Iterator<Item = &str> {
        self.messages.keys().map(String::as_str)
    }
    fn render(&self, key: &str, args: &[(&str, &str)]) -> Result<String, String> {
        let template = self.get(key).ok_or_else(|| format!("missing text key: {key}"))?;
        render_template(key, template, args)
    }
}

/// External catalogs with the paired, compiled-in fallback.
#[derive(Clone, Debug)]
pub struct TextBundle {
    default_language: String,
    external: HashMap<String, TextCatalog>,
    embedded: HashMap<String, TextCatalog>,
}

impl TextBundle {
    pub fn default_language(&self) -> &str {
        &self.default_language
    }
    pub fn embedded(default_language: &str) -> Result<Self, String> {
        Self::load(default_language, std::iter::empty())
    }
    pub fn load(default_language: &str, catalogs: impl IntoIterator<Item = (String, TextCatalog)>) -> Result<Self, String> {
        validate_language(default_language)?;
        let embedded = HashMap::from([
            ("zh-CN".to_owned(), TextCatalog::parse(ZH_CN)?),
            ("en".to_owned(), TextCatalog::parse(EN)?),
        ]);
        Self::validate_complete_pair(&embedded["zh-CN"], &embedded["en"])?;
        let mut external = HashMap::new();
        for (language, catalog) in catalogs {
            validate_language(&language)?;
            catalog.validate_language(&language)?;
            if external.insert(language.clone(), catalog).is_some() {
                return Err(format!("duplicate text catalog for language {language}"));
            }
        }
        if let (Some(zh), Some(en)) = (external.get("zh-CN"), external.get("en")) {
            zh.validate_compatible_with("zh-CN", en, "en")?;
        }
        Ok(Self {
            default_language: default_language.into(),
            external,
            embedded,
        })
    }
    /// Startup retains valid files and excludes a conflicting secondary locale. Reports are intended for one summary log.
    pub fn load_startup(
        default_language: &str,
        candidates: impl IntoIterator<Item = (String, Result<TextCatalog, String>)>,
    ) -> Result<(Self, Vec<String>), String> {
        validate_language(default_language)?;
        let mut accepted = Vec::new();
        let mut reports = Vec::new();
        for (language, candidate) in candidates {
            match candidate.and_then(|catalog| {
                catalog.validate_language(&language)?;
                Ok(catalog)
            }) {
                Ok(catalog) => accepted.push((language, catalog)),
                Err(error) => reports.push(format!("{language} rejected: {error}")),
            }
        }
        if let Some(default) = accepted
            .iter()
            .find(|(language, _)| language == default_language)
            .map(|(_, catalog)| catalog.clone())
        {
            accepted.retain(|(language, catalog)| {
                if language == default_language {
                    return true;
                }
                match default.validate_compatible_with(default_language, catalog, language) {
                    Ok(()) => true,
                    Err(error) => {
                        reports.push(format!("{language} rejected: {error}"));
                        false
                    }
                }
            });
        } else {
            reports.push(format!("{default_language} missing or invalid; embedded text will be used"));
        }
        Ok((Self::load(default_language, accepted)?, reports))
    }
    pub fn load_startup_for_config(
        default_language: &str,
        candidates: impl IntoIterator<Item = (String, Result<TextCatalog, String>)>,
        config: &Config,
    ) -> Result<(Self, Vec<String>), String> {
        let (mut bundle, mut reports) = Self::load_startup(default_language, candidates)?;
        let embedded_default = bundle.embedded.get(default_language).expect("supported locale");
        bundle.external.retain(|language, catalog| {
            if let Err(error) = validate_catalog_mode_values(config, language, catalog, embedded_default) {
                reports.push(format!("{language} rejected: {error}"));
                false
            } else {
                true
            }
        });
        Ok((bundle, reports))
    }
    /// Keys absent from this locale's external catalog and therefore resolved by fallback.
    pub fn missing_keys(&self, language: &str) -> Vec<String> {
        let Some(catalog) = self.external.get(language) else {
            return self
                .embedded
                .get(&self.default_language)
                .map(|c| c.keys().map(str::to_owned).collect())
                .unwrap_or_default();
        };
        self.embedded
            .get(&self.default_language)
            .into_iter()
            .flat_map(|c| c.keys())
            .filter(|key| catalog.get(key).is_none())
            .map(str::to_owned)
            .collect()
    }
    /// Resolve requested external text, then server-default external text, then embedded server-default text.
    pub fn render(&self, language: &str, key: &str, args: &[(&str, &str)]) -> Result<String, String> {
        if let Some(catalog) = self.external.get(language)
            && catalog.get(key).is_some()
        {
            return catalog.render(key, args);
        }
        if let Some(catalog) = self.external.get(&self.default_language)
            && catalog.get(key).is_some()
        {
            return catalog.render(key, args);
        }
        self.embedded
            .get(&self.default_language)
            .expect("supported bundle always contains embedded server-default text")
            .render(key, args)
    }
    /// Resolve a global label for a nominated server default, rather than falling
    /// back through the current server's different default language.
    pub fn render_default(&self, language: &str, key: &str, args: &[(&str, &str)]) -> Result<String, String> {
        validate_language(language)?;
        if let Some(catalog) = self.external.get(language)
            && catalog.get(key).is_some()
        {
            return catalog.render(key, args);
        }
        self.embedded[language].render(key, args)
    }

    /// Strict package validation: both catalogs must be complete and use matching placeholders.
    fn validate_complete_pair(zh_cn: &TextCatalog, en: &TextCatalog) -> Result<(), String> {
        if zh_cn.messages.keys().ne(en.messages.keys()) {
            let missing_en: Vec<_> = zh_cn.messages.keys().filter(|key| !en.messages.contains_key(*key)).cloned().collect();
            let missing_zh: Vec<_> = en.messages.keys().filter(|key| !zh_cn.messages.contains_key(*key)).cloned().collect();
            return Err(format!(
                "catalog keys differ (missing from en: {missing_en:?}; missing from zh-CN: {missing_zh:?})"
            ));
        }
        for (key, zh) in &zh_cn.messages {
            if placeholders(zh)? != placeholders(&en.messages[key])? {
                return Err(format!("{key}: zh-CN and en placeholder sets differ"));
            }
        }
        Ok(())
    }
    pub fn validate_complete_catalogs(zh_cn: &TextCatalog, en: &TextCatalog) -> Result<(), String> {
        Self::validate_complete_pair(zh_cn, en)?;
        let embedded_zh = TextCatalog::parse(ZH_CN)?;
        let embedded_en = TextCatalog::parse(EN)?;
        for (language, supplied, embedded) in [("zh-CN", zh_cn, &embedded_zh), ("en", en, &embedded_en)] {
            for key in embedded.keys() {
                let text = supplied
                    .get(key)
                    .ok_or_else(|| format!("{language} catalog is missing required built-in key {key:?}"))?;
                if placeholders(text)? != placeholders(embedded.get(key).expect("embedded key"))? {
                    return Err(format!("{language} catalog {key:?} placeholder set differs from built-in text"));
                }
            }
        }
        Ok(())
    }
    pub fn validate_config(&self, config: &Config) -> Result<(), String> {
        if self.default_language != config.default_language {
            return Err(format!(
                "text bundle default language {:?} does not match config {:?}",
                self.default_language, config.default_language
            ));
        }
        let embedded_default = self.embedded.get(&self.default_language).expect("supported default locale");
        let external_default = self.external.get(&self.default_language);
        for mode in &config.modes {
            validate_mode_keys(mode, external_default, embedded_default, &self.external)?;
        }
        if let Some(warmup) = &config.warmup {
            validate_mode_keys(&warmup.mode, external_default, embedded_default, &self.external)?;
        }
        Ok(())
    }
}

fn validate_mode_keys(
    mode: &Mode,
    external_default: Option<&TextCatalog>,
    embedded_default: &TextCatalog,
    external: &HashMap<String, TextCatalog>,
) -> Result<(), String> {
    for (field, key, legacy) in [
        ("name_key", mode.name_key.as_deref(), mode.name.as_str()),
        ("description_key", mode.description_key.as_deref(), mode.description.as_str()),
    ] {
        if !legacy.is_empty() && (legacy.trim().is_empty() || legacy.contains(['\r', '\n', '\0'])) {
            return Err(format!("{}: {field} text must be nonempty single-line text", mode.id));
        }
        if !legacy.is_empty() {
            continue;
        }
        if let Some(key) = key {
            let text = external_default
                .and_then(|catalog| catalog.get(key))
                .or_else(|| embedded_default.get(key))
                .ok_or_else(|| format!("{}: {field} references missing default-language text key {key:?}", mode.id))?;
            validate_display_text(text, mode, field, key, "server default")?;
            for (locale, external_catalog) in external {
                if let Some(text) = external_catalog.get(key) {
                    validate_display_text(text, mode, field, key, locale)?;
                }
            }
        }
    }
    Ok(())
}

fn validate_display_text(text: &str, mode: &Mode, field: &str, key: &str, locale: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.contains(['\r', '\n', '\0']) {
        return Err(format!(
            "{}: {field} text key {key:?} must be nonempty single-line text ({locale})",
            mode.id
        ));
    }
    if !placeholders(text)?.is_empty() {
        return Err(format!("{}: {field} text key {key:?} must not have placeholders ({locale})", mode.id));
    }
    Ok(())
}

fn validate_catalog_mode_values(config: &Config, language: &str, catalog: &TextCatalog, embedded_default: &TextCatalog) -> Result<(), String> {
    for mode in config.modes.iter().chain(config.warmup.iter().map(|warmup| &warmup.mode)) {
        for (field, key, legacy) in [
            ("name_key", mode.name_key.as_deref(), mode.name.as_str()),
            ("description_key", mode.description_key.as_deref(), mode.description.as_str()),
        ] {
            if !legacy.is_empty() {
                continue;
            }
            if let Some(key) = key {
                let value = if language == config.default_language {
                    catalog.get(key).or_else(|| embedded_default.get(key))
                } else {
                    catalog.get(key)
                };
                if let Some(value) = value {
                    validate_display_text(value, mode, field, key, language)?;
                }
            }
        }
    }
    Ok(())
}

fn validate_shared_placeholders(a: &TextCatalog, b: &TextCatalog, language_a: &str, language_b: &str) -> Result<(), String> {
    for key in a.messages.keys().filter(|key| b.messages.contains_key(*key)) {
        if placeholders(&a.messages[key])? != placeholders(&b.messages[key])? {
            return Err(format!("{key}: {language_a} and {language_b} placeholder sets differ"));
        }
    }
    Ok(())
}
fn validate_language(language: &str) -> Result<(), String> {
    if matches!(language, "zh-CN" | "en") {
        Ok(())
    } else {
        Err(format!("unsupported text language: {language:?}"))
    }
}
fn validate_key(key: &str) -> Result<(), String> {
    if key
        .split('.')
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'))
    {
        Ok(())
    } else {
        Err(format!("invalid text key {key:?}; use dotted ASCII identifiers"))
    }
}
fn placeholders(template: &str) -> Result<BTreeSet<String>, String> {
    let chars: Vec<char> = template.chars().collect();
    let mut names = BTreeSet::new();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '{' if chars.get(index + 1) == Some(&'{') => index += 2,
            '}' if chars.get(index + 1) == Some(&'}') => index += 2,
            '{' => {
                let start = index + 1;
                let mut end = start;
                while end < chars.len() && chars[end] != '}' {
                    end += 1;
                }
                if end == chars.len() {
                    return Err("unclosed placeholder".into());
                }
                let name: String = chars[start..end].iter().collect();
                let mut bytes = name.bytes();
                if !bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
                    || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                {
                    return Err(format!("invalid named placeholder {{{name}}}"));
                }
                names.insert(name);
                index = end + 1;
            }
            '}' => return Err("unescaped closing brace".into()),
            _ => index += 1,
        }
    }
    Ok(names)
}
fn render_template(key: &str, template: &str, args: &[(&str, &str)]) -> Result<String, String> {
    let required = placeholders(template)?;
    let supplied: BTreeSet<String> = args.iter().map(|(name, _)| (*name).to_owned()).collect();
    if required != supplied {
        return Err(format!("{key}: placeholder mismatch (expected {required:?}, got {supplied:?})"));
    }
    let values: HashMap<&str, &str> = args.iter().copied().collect();
    let chars: Vec<char> = template.chars().collect();
    let mut output = String::with_capacity(template.len());
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '{' if chars.get(index + 1) == Some(&'{') => {
                output.push('{');
                index += 2;
            }
            '}' if chars.get(index + 1) == Some(&'}') => {
                output.push('}');
                index += 2;
            }
            '{' => {
                let start = index + 1;
                let end = chars[start..]
                    .iter()
                    .position(|character| *character == '}')
                    .expect("validated placeholder")
                    + start;
                let name: String = chars[start..end].iter().collect();
                output.push_str(values[name.as_str()]);
                index = end + 1;
            }
            character => {
                output.push(character);
                index += 1;
            }
        }
    }
    Ok(output)
}
