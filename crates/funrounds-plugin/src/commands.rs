// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{bombs::WireColor, preferences::Knock};
use funrounds_core::NextMode;

pub const NAMES: &[&str] = &[
    "fr_status",
    "fr_list",
    "fr_next",
    "fr_reload",
    "fr_enable",
    "fr_info",
    "fr_help",
    "fr_knock",
    "fr_c4",
    "fr_suicide",
    "fr_nvg",
    "fr_wire",
    "fr_chicken",
    "fr_healthshot",
];

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Status,
    List,
    Next(NextMode),
    Reload,
    Enable(bool),
    Info,
    Help,
    C4,
    Suicide,
    Chicken,
    Healthshot,
    NightVision,
    Wire(WireColor),
    Knock(Option<Knock>),
}

impl Command {
    pub fn parse(args: &[String]) -> Result<Self, &'static str> {
        match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["fr_c4"] => Ok(Self::C4),
            ["fr_suicide"] => Ok(Self::Suicide),
            ["fr_chicken"] => Ok(Self::Chicken),
            ["fr_healthshot"] => Ok(Self::Healthshot),
            ["fr_nvg"] => Ok(Self::NightVision),
            ["fr_wire", color] => WireColor::parse(color).map(Self::Wire).ok_or("command.wire_usage"),
            ["fr_status"] => Ok(Self::Status),
            ["fr_list"] => Ok(Self::List),
            ["fr_info"] => Ok(Self::Info),
            ["fr_help"] => Ok(Self::Help),
            ["fr_knock"] => Ok(Self::Knock(None)),
            ["fr_knock", "0"] => Ok(Self::Knock(Some(Knock::Off))),
            ["fr_knock", "1"] => Ok(Self::Knock(Some(Knock::Always))),
            ["fr_knock", "2"] => Ok(Self::Knock(Some(Knock::Airborne))),
            ["fr_reload"] => Ok(Self::Reload),
            ["fr_enable", "0"] => Ok(Self::Enable(false)),
            ["fr_enable", "1"] => Ok(Self::Enable(true)),
            ["fr_next", "random"] => Ok(Self::Next(NextMode::Random)),
            ["fr_next", id] if !id.is_empty() => Ok(Self::Next(NextMode::Id((*id).into()))),
            _ => Err("command.usage"),
        }
    }

    pub fn permits_player(&self) -> bool {
        matches!(
            self,
            Self::Info
                | Self::Help
                | Self::Knock(_)
                | Self::C4
                | Self::Suicide
                | Self::Chicken
                | Self::Healthshot
                | Self::NightVision
                | Self::Wire(_)
        )
    }
}

pub fn client_executable(name: &str) -> bool {
    matches!(
        name,
        "fr_info" | "fr_help" | "fr_knock" | "fr_c4" | "fr_suicide" | "fr_chicken" | "fr_healthshot" | "fr_nvg" | "fr_wire"
    )
}

pub fn status_reply_parts(message: &str) -> impl Iterator<Item = &str> {
    // The recovery fixture exposed native single-reply truncation at 2047 bytes.
    message.lines().flat_map(|mut line| {
        std::iter::from_fn(move || {
            if line.is_empty() {
                return None;
            }
            let mut end = line.floor_char_boundary(1024.min(line.len()));
            if end < line.len()
                && let Some(separator) = line[..end].rfind(';')
            {
                end = separator + 1;
            }
            let (part, remaining) = line.split_at(end);
            line = remaining;
            Some(part)
        })
    })
}

pub fn parse_chat(args: &[String], chat_context: bool) -> Option<Result<Command, &'static str>> {
    let text = if chat_context {
        args.join(" ")
    } else if matches!(args.first().map(String::as_str), Some("say" | "say_team")) {
        args[1..].join(" ").trim_matches('"').to_owned()
    } else {
        return None;
    };
    let mut words = text.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
    let first = words.first()?;
    let name = match (chat_context, first.as_str()) {
        (true, "info" | "frinfo") | (false, "!info" | "!frinfo") => "fr_info",
        (true, "help" | "frhelp") | (false, "!help" | "!frhelp") => "fr_help",
        (true, "c4") | (false, "!c4") => "fr_c4",
        (true, "suicide") | (false, "!suicide") => "fr_suicide",
        (true, "chicken") | (false, "!chicken") => "fr_chicken",
        (true, "healthshot") | (false, "!healthshot") => "fr_healthshot",
        (true, "nvg") | (false, "!nvg") => "fr_nvg",
        (true, "wire") | (false, "!wire") => "fr_wire",
        (true, "knock") | (false, "!knock") => "fr_knock",
        _ => return None,
    };
    words[0] = name.into();
    Some(Command::parse(&words))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_replies_keep_all_long_unicode_diagnostics_without_native_truncation() {
        let message = format!(
            "faults=Global:[{}];Mechanism(Blue):[{}]",
            "恢复错误".repeat(500),
            "write failed;".repeat(200)
        );
        let parts = status_reply_parts(&message).collect::<Vec<_>>();
        assert!(parts.len() > 2);
        assert!(parts.iter().all(|part| !part.is_empty() && part.len() <= 1024));
        assert_eq!(parts.concat(), message);
        assert_eq!(
            status_reply_parts("recovery=Stopped\nfaults=Global:[failure]\nexcluded=sample:[equipment]").collect::<Vec<_>>(),
            ["recovery=Stopped", "faults=Global:[failure]", "excluded=sample:[equipment]",]
        );
    }

    #[test]
    fn chat_aliases_accept_player_commands_and_reject_console_text() {
        let args = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
        assert_eq!(parse_chat(&args(&["frinfo"]), true), Some(Ok(Command::Info)));
        assert_eq!(parse_chat(&args(&["say", "!frhelp"]), false), Some(Ok(Command::Help)));
        assert_eq!(
            parse_chat(&args(&["say_team", "!knock 0"]), false),
            Some(Ok(Command::Knock(Some(Knock::Off))))
        );
        assert_eq!(parse_chat(&args(&["knock"]), true), Some(Ok(Command::Knock(None))));
        for words in [vec!["say", "hello"], vec!["say", "!fr_enable 0"], vec!["fr_enable", "0"], vec!["frinfo"]] {
            assert!(parse_chat(&args(&words), false).is_none());
        }
        for words in [vec!["fr_info"], vec!["fr_reload"], vec!["knock;quit"]] {
            assert!(parse_chat(&args(&words), true).is_none());
        }
        assert!(parse_chat(&args(&["say", "!knock 1;quit"]), false).unwrap().is_err());
        for name in ["fr_help", "fr_knock"] {
            assert!(client_executable(name));
            assert!(Command::parse(&args(&[name])).unwrap().permits_player());
        }
        assert_eq!(parse(&["fr_knock", "2"]), Ok(Command::Knock(Some(Knock::Airborne))));
        for words in [vec!["fr_knock", "1", "extra"], vec!["fr_help", "extra"]] {
            assert!(Command::parse(&args(&words)).is_err());
        }
    }

    #[test]
    fn localized_help_lists_each_player_command_on_its_own_line() {
        for language in ["zh-CN", "en"] {
            let texts = funrounds_core::TextBundle::embedded(language).unwrap();
            let help = texts.render(language, "help.body", &[]).unwrap();
            let lines = help.lines().collect::<Vec<_>>();
            for command in [
                "fr_info",
                "fr_help",
                "fr_knock",
                "fr_c4",
                "fr_suicide",
                "fr_chicken",
                "fr_healthshot",
                "fr_nvg",
                "fr_wire",
            ] {
                assert_eq!(
                    lines.iter().filter(|line| line.starts_with(&format!("{command} "))).count(),
                    1,
                    "{language}: {command}"
                );
            }
            assert_eq!(lines.len(), 9);
        }
    }

    fn parse(args: &[&str]) -> Result<Command, &'static str> {
        Command::parse(&args.iter().map(|arg| (*arg).into()).collect::<Vec<_>>())
    }

    #[test]
    fn management_commands_reject_ambiguous_arguments() {
        for args in [
            vec![],
            vec!["fr_enable"],
            vec!["fr_enable", "true"],
            vec!["fr_enable", "2"],
            vec!["fr_status", "extra"],
            vec!["fr_next", ""],
        ] {
            assert!(parse(&args).is_err());
        }
        assert_eq!(parse(&["fr_enable", "0"]), Ok(Command::Enable(false)));
        assert_eq!(parse(&["fr_enable", "1"]), Ok(Command::Enable(true)));
        assert_eq!(parse(&["fr_next", "random"]), Ok(Command::Next(NextMode::Random)));
        assert_eq!(parse(&["fr_next", "example"]), Ok(Command::Next(NextMode::Id("example".into()))));
    }

    #[test]
    fn players_cannot_execute_round_management_commands() {
        assert!(parse(&["fr_info"]).unwrap().permits_player());
        assert!(client_executable("fr_info"));
        for args in [
            vec!["fr_status"],
            vec!["fr_list"],
            vec!["fr_enable", "1"],
            vec!["fr_reload"],
            vec!["fr_next", "random"],
        ] {
            assert!(!client_executable(args[0]));
            assert!(!parse(&args).unwrap().permits_player());
        }
    }
}
