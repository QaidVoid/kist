//! The single table of user-facing commands.
//!
//! The palette, the help overlay, and the footer hints are all generated from
//! here, so a command cannot appear in one and be missing, differently
//! labelled, or bound to a stale key in another.

/// Every action a user can invoke by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandId {
    Add,
    AddWithOptions,
    Search,
    OpenDetails,
    Pause,
    Resume,
    Remove,
    AttachWebSeed,
    Filter,
    ClearFilter,
    Limits,
    ToggleMark,
    ClearMarks,
    MarkAll,
    SortByName,
    SortByState,
    SortByProgress,
    SortBySpeed,
    ReverseSort,
    Help,
    Quit,
}

/// Where a command belongs in the help overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Torrents,
    Selection,
    View,
    App,
}

impl Group {
    /// Heading shown above the group.
    pub fn label(self) -> &'static str {
        match self {
            Group::Torrents => "Torrents",
            Group::Selection => "Selection",
            Group::View => "View",
            Group::App => "Application",
        }
    }

    /// Display order.
    pub fn all() -> [Group; 4] {
        [Group::Torrents, Group::Selection, Group::View, Group::App]
    }
}

/// One command, as the user sees it.
#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    pub id: CommandId,
    /// What the command does, in the user's words.
    pub label: &'static str,
    /// A terse form for the footer, where room is scarce.
    pub short: &'static str,
    /// Extra words the palette matches on, so a command can be found by
    /// intent rather than by remembering its exact name.
    pub keywords: &'static str,
    /// The key that runs it, when it has one.
    pub key: Option<&'static str>,
    pub group: Group,
    /// Whether the command acts on a torrent, and so needs one to exist.
    pub needs_torrent: bool,
    /// Whether the footer must keep this hint even when space is short.
    pub essential: bool,
}

/// Every command, in the order the footer prefers to show them.
pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: CommandId::Add,
        label: "Add a torrent",
        short: "add",
        keywords: "new magnet file url open",
        key: Some("a"),
        group: Group::Torrents,
        needs_torrent: false,
        essential: true,
    },
    CommandSpec {
        id: CommandId::Help,
        label: "Show keys",
        short: "help",
        keywords: "help keys bindings shortcuts",
        key: Some("?"),
        group: Group::App,
        needs_torrent: false,
        essential: true,
    },
    CommandSpec {
        id: CommandId::Quit,
        label: "Quit kist",
        short: "quit",
        keywords: "exit close leave",
        key: Some("q"),
        group: Group::App,
        needs_torrent: false,
        essential: true,
    },
    CommandSpec {
        id: CommandId::OpenDetails,
        label: "Open details",
        short: "details",
        keywords: "inspect info files peers trackers",
        key: Some("i"),
        group: Group::View,
        needs_torrent: true,
        essential: false,
    },
    CommandSpec {
        id: CommandId::Pause,
        label: "Pause",
        short: "pause",
        keywords: "stop halt suspend",
        key: Some("p"),
        group: Group::Torrents,
        needs_torrent: true,
        essential: false,
    },
    CommandSpec {
        id: CommandId::Resume,
        label: "Resume",
        short: "resume",
        keywords: "start continue unpause",
        key: Some("r"),
        group: Group::Torrents,
        needs_torrent: true,
        essential: false,
    },
    CommandSpec {
        id: CommandId::Remove,
        label: "Remove",
        short: "remove",
        keywords: "delete forget drop",
        key: Some("d"),
        group: Group::Torrents,
        needs_torrent: true,
        essential: false,
    },
    CommandSpec {
        id: CommandId::ToggleMark,
        label: "Mark or unmark",
        short: "mark",
        keywords: "select multi bulk choose tick",
        key: Some("space"),
        group: Group::Selection,
        needs_torrent: true,
        essential: false,
    },
    CommandSpec {
        id: CommandId::Filter,
        label: "Filter by name",
        short: "filter",
        keywords: "search find narrow",
        key: Some("/"),
        group: Group::View,
        needs_torrent: false,
        essential: false,
    },
    CommandSpec {
        id: CommandId::Search,
        label: "Search indexers",
        short: "search",
        keywords: "find download discover apibay",
        key: Some("f"),
        group: Group::Torrents,
        needs_torrent: false,
        essential: false,
    },
    CommandSpec {
        id: CommandId::AttachWebSeed,
        label: "Attach a web seed",
        short: "web seed",
        keywords: "http mirror source url bep19",
        key: Some("w"),
        group: Group::Torrents,
        needs_torrent: true,
        essential: false,
    },
    CommandSpec {
        id: CommandId::Limits,
        label: "Set rate limits",
        short: "limits",
        keywords: "speed cap throttle bandwidth",
        key: Some("L"),
        group: Group::App,
        needs_torrent: false,
        essential: false,
    },
    CommandSpec {
        id: CommandId::AddWithOptions,
        label: "Add with options",
        short: "add+",
        keywords: "new paused folder files choose",
        key: Some("A"),
        group: Group::Torrents,
        needs_torrent: false,
        essential: false,
    },
    CommandSpec {
        id: CommandId::MarkAll,
        label: "Mark everything shown",
        short: "mark all",
        keywords: "select all bulk every",
        key: None,
        group: Group::Selection,
        needs_torrent: true,
        essential: false,
    },
    CommandSpec {
        id: CommandId::ClearMarks,
        label: "Clear marks",
        short: "unmark",
        keywords: "deselect none unmark",
        key: Some("esc"),
        group: Group::Selection,
        needs_torrent: false,
        essential: false,
    },
    CommandSpec {
        id: CommandId::ClearFilter,
        label: "Clear the filter",
        short: "unfilter",
        keywords: "reset show all",
        key: None,
        group: Group::View,
        needs_torrent: false,
        essential: false,
    },
    CommandSpec {
        id: CommandId::SortByName,
        label: "Sort by name",
        short: "sort name",
        keywords: "order alphabetical",
        key: None,
        group: Group::View,
        needs_torrent: false,
        essential: false,
    },
    CommandSpec {
        id: CommandId::SortByState,
        label: "Sort by state",
        short: "sort state",
        keywords: "order status",
        key: None,
        group: Group::View,
        needs_torrent: false,
        essential: false,
    },
    CommandSpec {
        id: CommandId::SortByProgress,
        label: "Sort by progress",
        short: "progress",
        keywords: "order percent complete",
        key: None,
        group: Group::View,
        needs_torrent: false,
        essential: false,
    },
    CommandSpec {
        id: CommandId::SortBySpeed,
        label: "Sort by speed",
        short: "sort speed",
        keywords: "order rate fastest",
        key: None,
        group: Group::View,
        needs_torrent: false,
        essential: false,
    },
    CommandSpec {
        id: CommandId::ReverseSort,
        label: "Reverse sort direction",
        short: "reverse",
        keywords: "order flip ascending descending",
        key: Some("S"),
        group: Group::View,
        needs_torrent: false,
        essential: false,
    },
];

/// Commands runnable right now, filtered by a palette query.
///
/// A command that acts on a torrent is withheld when there is none, so the
/// palette never offers an action that would quietly do nothing.
pub fn matching(query: &str, has_torrent: bool) -> Vec<&'static CommandSpec> {
    // Every typed word must appear somewhere, so "sort speed" finds "Sort by
    // speed" and word order does not have to match what the author chose.
    let tokens: Vec<String> = query.split_whitespace().map(|t| t.to_lowercase()).collect();
    COMMANDS
        .iter()
        .filter(|c| has_torrent || !c.needs_torrent)
        .filter(|c| {
            let haystack = format!("{} {}", c.label.to_lowercase(), c.keywords);
            tokens.iter().all(|token| haystack.contains(token.as_str()))
        })
        .collect()
}

/// Commands belonging to `group` that can run right now.
pub fn in_group(group: Group, has_torrent: bool) -> Vec<&'static CommandSpec> {
    COMMANDS
        .iter()
        .filter(|c| c.group == group)
        .filter(|c| has_torrent || !c.needs_torrent)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_has_a_label_and_keywords() {
        for spec in COMMANDS {
            assert!(!spec.label.is_empty(), "{:?} has no label", spec.id);
            assert!(!spec.short.is_empty(), "{:?} has no short label", spec.id);
            assert!(
                spec.short.len() <= 12,
                "{:?} short label is too long for a footer",
                spec.id
            );
            assert!(!spec.keywords.is_empty(), "{:?} has no keywords", spec.id);
            assert_eq!(
                spec.keywords.to_lowercase(),
                spec.keywords,
                "{:?} keywords must be lowercase to match",
                spec.id
            );
        }
    }

    #[test]
    fn command_ids_are_unique() {
        let mut seen = Vec::new();
        for spec in COMMANDS {
            assert!(!seen.contains(&spec.id), "{:?} listed twice", spec.id);
            seen.push(spec.id);
        }
    }

    #[test]
    fn matching_finds_commands_by_intent() {
        let by_word = matching("mirror", true);
        assert_eq!(by_word.len(), 1);
        assert_eq!(by_word[0].id, CommandId::AttachWebSeed);

        let by_label = matching("quit", true);
        assert_eq!(by_label[0].id, CommandId::Quit);
    }

    #[test]
    fn torrent_commands_are_withheld_without_a_torrent() {
        let without = matching("", false);
        assert!(
            without.iter().all(|c| !c.needs_torrent),
            "an empty list must not offer torrent actions"
        );
        assert!(
            without.iter().any(|c| c.id == CommandId::Add),
            "adding must still be offered"
        );
        assert!(matching("", true).iter().any(|c| c.needs_torrent));
    }

    #[test]
    fn multi_word_queries_match_in_any_order() {
        let hits = matching("sort speed", true);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, CommandId::SortBySpeed);
        assert_eq!(matching("speed sort", true)[0].id, CommandId::SortBySpeed);
    }

    #[test]
    fn unmatched_queries_return_nothing() {
        assert!(matching("zzzznotacommand", true).is_empty());
    }

    #[test]
    fn every_sort_key_is_reachable_by_name() {
        for id in [
            CommandId::SortByName,
            CommandId::SortByState,
            CommandId::SortByProgress,
            CommandId::SortBySpeed,
        ] {
            assert!(
                COMMANDS.iter().any(|c| c.id == id),
                "{id:?} must be pickable directly rather than by cycling"
            );
        }
    }
}
