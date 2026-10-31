// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use funrounds_core::{Config, TextBundle, TextCatalog};
use std::{env, fs, path::Path, process::ExitCode};

fn validate_file(path: &Path) -> Result<(), String> {
    let source = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Config::parse(&source).map(|_| ()).map_err(|error| format!("{}: {error}", path.display()))
}

fn validate_bundle(config: &Config, directory: &Path, allow_fallback: bool) -> Result<(), String> {
    let zh_path = directory.join("zh-CN.toml");
    let en_path = directory.join("en.toml");
    let catalogs = if allow_fallback {
        let mut catalogs = Vec::new();
        for (language, path) in [("zh-CN", &zh_path), ("en", &en_path)] {
            if let Some(catalog) = read_catalog_if_present(path)? {
                catalogs.push((language.to_owned(), catalog));
            }
        }
        catalogs
    } else {
        let zh = read_catalog(&zh_path)?;
        let en = read_catalog(&en_path)?;
        TextBundle::validate_complete_catalogs(&zh, &en).map_err(|error| format!("{}: {error}", directory.display()))?;
        vec![("zh-CN".into(), zh), ("en".into(), en)]
    };
    let bundle = TextBundle::load(config.default_language.as_str(), catalogs)?;
    bundle.validate_config(config)
}

fn read_catalog(path: &Path) -> Result<TextCatalog, String> {
    let source = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    TextCatalog::parse(&source).map_err(|error| format!("{}: {error}", path.display()))
}

fn read_catalog_if_present(path: &Path) -> Result<Option<TextCatalog>, String> {
    match fs::read_to_string(path) {
        Ok(source) => TextCatalog::parse(&source)
            .map(Some)
            .map_err(|error| format!("{}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args_os();
    let program = arguments.next().unwrap_or_default();
    let path = arguments.next().ok_or_else(|| usage(&program))?;
    let next = arguments.next();
    if next.is_none() {
        return validate_file(Path::new(&path));
    }
    let usage = usage(&program);
    if next.as_deref() != Some(std::ffi::OsStr::new("--i18n-dir")) {
        return Err(usage);
    }
    let directory = arguments.next().ok_or_else(|| usage.clone())?;
    let allow_fallback = match arguments.next() {
        None => false,
        Some(flag) if flag == "--allow-fallback" => true,
        Some(_) => return Err(usage),
    };
    if arguments.next().is_some() {
        return Err(usage);
    }
    let config_path = Path::new(&path);
    let source = fs::read_to_string(config_path).map_err(|error| format!("{}: {error}", config_path.display()))?;
    let config = Config::parse(&source).map_err(|error| format!("{}: {error}", config_path.display()))?;
    validate_bundle(&config, Path::new(&directory), allow_fallback)
}

fn usage(program: &std::ffi::OsStr) -> String {
    format!(
        "usage: {} <config.toml> [--i18n-dir <directory> [--allow-fallback]]",
        Path::new(program).display()
    )
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
