// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

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
];
pub const HELP: &str = "fr_info / !frinfo — 查看当前模式\nfr_help / !frhelp — 查看帮助\nfr_knock [0|1] / !knock [0|1] — 设置开枪自推（无参数切换）";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Status,
    List,
    Next(NextMode),
    Reload,
    Enable(bool),
    Info,
    Help,
    Knock(Option<bool>),
}

impl Command {
    pub fn parse(args: &[String]) -> Result<Self, &'static str> {
        match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["fr_status"] => Ok(Self::Status),
            ["fr_list"] => Ok(Self::List),
            ["fr_info"] => Ok(Self::Info),
            ["fr_help"] => Ok(Self::Help),
            ["fr_knock"] => Ok(Self::Knock(None)),
            ["fr_knock", "0"] => Ok(Self::Knock(Some(false))),
            ["fr_knock", "1"] => Ok(Self::Knock(Some(true))),
            ["fr_reload"] => Ok(Self::Reload),
            ["fr_enable", "0"] => Ok(Self::Enable(false)),
            ["fr_enable", "1"] => Ok(Self::Enable(true)),
            ["fr_next", "random"] => Ok(Self::Next(NextMode::Random)),
            ["fr_next", id] if !id.is_empty() => Ok(Self::Next(NextMode::Id((*id).into()))),
            _ => Err("usage: fr_status | fr_list | fr_next <id|random> | fr_reload | fr_enable <0|1> | fr_info | fr_help | fr_knock [0|1]"),
        }
    }

    pub fn permits_player(&self) -> bool {
        matches!(self, Self::Info | Self::Help | Self::Knock(_))
    }
}

pub fn client_executable(name: &str) -> bool {
    matches!(name, "fr_info" | "fr_help" | "fr_knock")
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
        (true, "frinfo") | (false, "!frinfo") => "fr_info",
        (true, "frhelp") | (false, "!frhelp") => "fr_help",
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
            status_reply_parts("recovery=Stopped\nfaults=Global:[failure]\nexcluded=85:[classic]").collect::<Vec<_>>(),
            ["recovery=Stopped", "faults=Global:[failure]", "excluded=85:[classic]",]
        );
    }

    #[test]
    fn chat_aliases_accept_only_the_three_player_commands_and_never_console_text() {
        let args = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
        assert_eq!(parse_chat(&args(&["frinfo"]), true), Some(Ok(Command::Info)));
        assert_eq!(parse_chat(&args(&["say", "!frhelp"]), false), Some(Ok(Command::Help)));
        assert_eq!(parse_chat(&args(&["say_team", "!knock 0"]), false), Some(Ok(Command::Knock(Some(false)))));
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
        for words in [vec!["fr_knock", "2"], vec!["fr_knock", "1", "extra"], vec!["fr_help", "extra"]] {
            assert!(Command::parse(&args(&words)).is_err());
        }
    }

    #[test]
    fn help_shows_each_player_command_on_its_own_line() {
        assert_eq!(
            HELP.lines().collect::<Vec<_>>(),
            [
                "fr_info / !frinfo — 查看当前模式",
                "fr_help / !frhelp — 查看帮助",
                "fr_knock [0|1] / !knock [0|1] — 设置开枪自推（无参数切换）",
            ]
        );
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
    fn players_can_only_read_information() {
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
