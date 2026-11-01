//! Slash commands: their definitions, and a command interaction as
//! [`InteractionKind::Command`] for the hub, which answers.

use domovoi_core::InteractionKind;
use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::application_command::{
    CommandData, CommandOptionValue,
};
use twilight_util::builder::command::{CommandBuilder, StringBuilder};

/// The command names, in the order they are registered.
pub const NAMES: [&str; 4] = ["status", "incidents", "mute", "digest"];

/// The `/mute` durations.
pub const MUTE_DURATIONS: [&str; 3] = ["1h", "4h", "24h"];

/// Every command the bot registers in the incident channel's server.
#[must_use]
pub fn definitions() -> Vec<Command> {
    let mute = CommandBuilder::new("mute", "Mutes an incident", CommandType::ChatInput)
        .option(
            StringBuilder::new("incident", "The incident's short id, from its card").required(true),
        )
        .option(
            StringBuilder::new("duration", "For how long")
                .required(true)
                .choices(MUTE_DURATIONS.map(|d| (d, d))),
        )
        .build();
    vec![
        CommandBuilder::new(
            "status",
            "Hosts and their open incidents",
            CommandType::ChatInput,
        )
        .build(),
        CommandBuilder::new(
            "incidents",
            "Open incidents, newest first",
            CommandType::ChatInput,
        )
        .build(),
        mute,
        CommandBuilder::new("digest", "Sends today's digest now", CommandType::ChatInput).build(),
    ]
}

/// A command interaction as the hub sees it: the name and its string arguments, in the
/// order the command declares them. `None` for commands that are not ours.
#[must_use]
pub fn command_kind(data: &CommandData) -> Option<InteractionKind> {
    if !NAMES.contains(&data.name.as_str()) {
        return None;
    }
    let value = |name: &str| {
        data.options
            .iter()
            .find_map(|o| match (&o.value, o.name == name) {
                (CommandOptionValue::String(s), true) => Some(s.clone()),
                _ => None,
            })
    };
    let args = match data.name.as_str() {
        "mute" => vec![value("incident")?, value("duration")?],
        _ => Vec::new(),
    };
    Some(InteractionKind::Command {
        name: data.name.clone(),
        args,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_defined_with_the_mute_choices() {
        let defs = definitions();
        let names: Vec<&str> = defs.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, NAMES);
        let mute = serde_json::to_value(&defs[2]).unwrap();
        let choices: Vec<&str> = mute["options"][1]["choices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["value"].as_str().unwrap())
            .collect();
        assert_eq!(choices, MUTE_DURATIONS);
    }
}
