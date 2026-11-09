// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use funrounds_core::{Config, TextBundle, TextCatalog};
use std::{env, fs, path::Path, process::ExitCode};

fn validate_file(path: &Path) -> Result<(), String> {
    let source = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Config::parse(&source).map(|_| ()).map_err(|error| format!("{}: {error}", path.display()))
}

fn validate_bundle(config: &Config, directory: &Path, allow_fallback: bool) -> Result<(), String> {
    let mut catalogs = Vec::new();
    match fs::read_dir(directory) {
        Ok(entries) => {
            for entry in entries {
                let path = entry.map_err(|error| format!("{}: {error}", directory.display()))?.path();
                if path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
                    && fs::metadata(&path).map_err(|error| format!("{}: {error}", path.display()))?.is_file()
                {
                    let language = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                    let source = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
                    let catalog = TextCatalog::parse_for_language(&source, &language).map_err(|error| format!("{}: {error}", path.display()))?;
                    catalogs.push((language, catalog));
                }
            }
        }
        Err(error) if allow_fallback && error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{}: {error}", directory.display())),
    }
    catalogs.sort_by(|(a, _), (b, _)| a.cmp(b));
    let bundle = TextBundle::load(config.default_language.as_str(), catalogs)?;
    if !allow_fallback {
        bundle
            .validate_complete_catalogs()
            .map_err(|error| format!("{}: {error}", directory.display()))?;
    }
    bundle.validate_config(config)
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
