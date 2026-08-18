//! Layout assertions covering every mode at the sizes the UI claims to support.

use super::harness::{self, assert_within_width, render_to_text, sizes};
use crate::app::{App, DetailTab, Mode};
use crate::model::{
    AggregateStats, DetailFile, DetailSnapshot, PeerRow, RowState, Snapshot, TorrentRow,
    WebSeedRow, WebSeedState,
};

/// A torrent row with deliberately awkward values: a long name, and a size and
/// speed in the four-digit range that produces the widest formatted strings.
fn row(id: usize, name: &str, state: RowState, finished: bool) -> TorrentRow {
    TorrentRow {
        id,
        name: name.to_string(),
        total_bytes: 1024 * 1000,
        progress_bytes: 1024 * 372,
        uploaded_bytes: 1024 * 96,
        finished,
        down_speed: 1024 * 1000,
        up_speed: 1024 * 1000,
        eta: Some(std::time::Duration::from_secs(3671)),
        peers: 24,
        state,
        error: (state == RowState::Error).then(|| "tracker returned 404".to_string()),
    }
}

fn populated() -> App {
    let mut app = App::new();
    app.update_snapshot(Snapshot::from_rows(vec![
        row(
            0,
            "A Torrent With A Rather Long Name Indeed.2026.mkv",
            RowState::Live,
            false,
        ),
        row(1, "ubuntu-24.04.3-desktop-amd64.iso", RowState::Live, true),
        row(2, "paused-thing", RowState::Paused, false),
        row(3, "broken-thing", RowState::Error, false),
        row(4, "checking-thing", RowState::Initializing, false),
    ]));
    app
}

fn detail() -> DetailSnapshot {
    DetailSnapshot {
        infohash: "abba124763b987366fe66f8461a48e4c8d4482d2".to_string(),
        state: RowState::Live,
        total_bytes: 1024 * 1000,
        progress_bytes: 1024 * 372,
        uploaded_bytes: 1024 * 96,
        down_speed: 1024 * 1000,
        up_speed: 1024 * 1000,
        eta: Some(std::time::Duration::from_secs(3671)),
        finished: false,
        peers: 24,
        files: (0..6)
            .map(|i| DetailFile {
                name: format!("subdir with spaces/file number {i} with a long name.bin"),
                size: 1024 * 1000,
                have: 1024 * 372,
                included: i % 2 == 0,
            })
            .collect(),
        peer_rows: (0..4)
            .map(|i| PeerRow {
                addr: format!("192.168.100.{i}:51413"),
                state: "live".to_string(),
                fetched_bytes: 1024 * 1000,
                web_seed: i == 0,
            })
            .collect(),
        trackers: vec![
            "http://tracker.example.org:6969/announce".to_string(),
            "udp://tracker.example.net:1337/announce".to_string(),
        ],
        web_seeds: vec![WebSeedRow {
            url: "https://example.com/a/rather/long/path/to/the/files/".to_string(),
            state: WebSeedState::Active,
            served_bytes: 1024 * 1000,
            error: None,
        }],
        pieces: Some((0..64).map(|i| i % 3 == 0).collect()),
    }
}

/// A named mode paired with a builder for the state it needs to render.
type ModeCase = (&'static str, Box<dyn Fn() -> App>);

/// Every mode, with whatever state that mode needs to render meaningfully.
fn modes() -> Vec<ModeCase> {
    vec![
        ("list", Box::new(populated)),
        (
            "list empty",
            Box::new(|| {
                let mut app = App::new();
                app.update_snapshot(Snapshot {
                    rows: Vec::new(),
                    aggregate: AggregateStats::default(),
                });
                app
            }),
        ),
        (
            "list filtered empty",
            Box::new(|| {
                let mut app = populated();
                app.filter = Some("nothing matches this".to_string());
                app
            }),
        ),
        (
            "add bar",
            Box::new(|| {
                let mut app = populated();
                app.mode = Mode::AddBar;
                app.input = "magnet:?xt=urn:btih:".repeat(6);
                app
            }),
        ),
        (
            "filter",
            Box::new(|| {
                let mut app = populated();
                app.mode = Mode::Filter;
                app
            }),
        ),
        (
            "limits",
            Box::new(|| {
                let mut app = populated();
                app.mode = Mode::Limits;
                app
            }),
        ),
        (
            "help",
            Box::new(|| {
                let mut app = populated();
                app.mode = Mode::Help;
                app
            }),
        ),
        (
            "confirm remove",
            Box::new(|| {
                let mut app = populated();
                app.mode = Mode::ConfirmRemove { id: 0 };
                app
            }),
        ),
        (
            "search input",
            Box::new(|| {
                let mut app = populated();
                app.mode = Mode::SearchInput;
                app
            }),
        ),
        (
            "search results",
            Box::new(|| {
                let mut app = populated();
                app.mode = Mode::SearchResults;
                app.search_query = "ubuntu".to_string();
                app
            }),
        ),
        (
            "web seed prompt",
            Box::new(|| {
                let mut app = populated();
                app.mode = Mode::WebSeedPrompt {
                    id: 0,
                    from_detail: false,
                };
                app
            }),
        ),
        (
            "palette",
            Box::new(|| {
                let mut app = populated();
                app.mode = Mode::Palette;
                app
            }),
        ),
        (
            "palette filtered",
            Box::new(|| {
                let mut app = populated();
                app.mode = Mode::Palette;
                app.input = "sort".to_string();
                app
            }),
        ),
        (
            "palette no match",
            Box::new(|| {
                let mut app = populated();
                app.mode = Mode::Palette;
                app.input = "zzzz".to_string();
                app
            }),
        ),
        (
            "web seed prompt invalid",
            Box::new(|| {
                let mut app = populated();
                app.mode = Mode::WebSeedPrompt {
                    id: 0,
                    from_detail: false,
                };
                app.input = "not-a-url".to_string();
                app.cursor = app.input.len();
                app
            }),
        ),
    ]
}

/// Every detail tab, which all share one mode but render very differently.
fn detail_tabs() -> Vec<(&'static str, DetailTab)> {
    vec![
        ("detail overview", DetailTab::Overview),
        ("detail files", DetailTab::Files),
        ("detail peers", DetailTab::Peers),
        ("detail trackers", DetailTab::Trackers),
        ("detail sources", DetailTab::Sources),
    ]
}

#[test]
fn every_mode_fits_every_supported_width() {
    for (name, build) in modes() {
        for size in sizes() {
            let mut app = build();
            assert_within_width(&mut app, size, name);
        }
    }
}

#[test]
fn every_detail_tab_fits_every_supported_width() {
    for (name, tab) in detail_tabs() {
        for size in sizes() {
            let mut app = populated();
            app.mode = Mode::Detail { id: 0 };
            app.detail_tab = tab;
            app.set_detail(Some(detail()));
            assert_within_width(&mut app, size, name);
        }
    }
}

/// Text every overlay must still show at the smallest supported terminal.
///
/// Width assertions alone miss this: an overlay can fit its width perfectly
/// and still push its most important content off the bottom of the screen.
fn required_text() -> Vec<(&'static str, Mode, Vec<&'static str>)> {
    vec![
        (
            "confirm remove",
            Mode::ConfirmRemove { id: 0 },
            // Both outcomes must be visible, or the user is offered a choice
            // they cannot see. The destructive one especially.
            vec!["forget", "delete", "cancel"],
        ),
        ("limits", Mode::Limits, vec!["Download", "Upload"]),
        ("help", Mode::Help, vec!["Quit", "close"]),
    ]
}

#[test]
fn prompts_complain_while_you_type() {
    let mut app = populated();
    app.mode = Mode::WebSeedPrompt {
        id: 0,
        from_detail: false,
    };
    app.input = "not-a-url".to_string();
    app.cursor = app.input.len();
    let text = render_to_text(&mut app, harness::NARROW);
    assert!(
        text.contains("not a valid URL"),
        "unparseable input should be called out:\n{text}"
    );

    // A parseable URL with the wrong scheme names what is accepted instead.
    app.input = "ftp://example.com/a.bin".to_string();
    app.cursor = app.input.len();
    let text = render_to_text(&mut app, harness::NARROW);
    assert!(
        text.contains("http"),
        "a wrong scheme should say what is expected:\n{text}"
    );

    // A valid URL leaves the hint line showing how to cancel instead.
    app.input = "https://example.com/files/".to_string();
    app.cursor = app.input.len();
    let text = render_to_text(&mut app, harness::NARROW);
    assert!(
        text.contains("cancel"),
        "expected the dismissal hint:\n{text}"
    );
}

#[test]
fn every_overlay_says_how_to_leave() {
    for (name, mode, _) in required_text() {
        let mut app = populated();
        app.mode = mode;
        let text = render_to_text(&mut app, harness::NARROW);
        assert!(
            text.contains("esc") || text.contains("cancel") || text.contains("close"),
            "{name} does not say how to dismiss it:\n{text}"
        );
    }
}

#[test]
fn an_unbounded_value_ellipsizes_rather_than_clipping() {
    // Peer counts have no formatter bound, so a wide one must show it lost
    // characters instead of quietly dropping them.
    let mut app = populated();
    app.snapshot.rows[0].peers = 1_234_567;
    let text = render_to_text(&mut app, harness::WIDE);
    assert!(
        text.contains('\u{2026}'),
        "an over-wide value should be ellipsized:\n{text}"
    );
}

#[test]
fn every_state_is_distinguishable_by_colour() {
    // A row's state has to read at a glance from its name, not only from its
    // one-character glyph. This is the assertion that catches a state quietly
    // losing its colour.
    let mut app = populated();
    let name_colour =
        |app: &mut App, needle: &str| harness::color_of_text(app, harness::WIDE, needle);

    assert_eq!(
        name_colour(&mut app, "ubuntu"),
        crate::ui::theme::OK,
        "a finished torrent must read as done"
    );
    assert_eq!(
        name_colour(&mut app, "broken-thing"),
        crate::ui::theme::ERROR,
        "an errored torrent must stand out"
    );
    assert_eq!(
        name_colour(&mut app, "paused-thing"),
        crate::ui::theme::TEXT_MUTED,
        "a paused torrent must recede"
    );
    assert_eq!(
        name_colour(&mut app, "checking-thing"),
        crate::ui::theme::ACCENT,
        "an initializing torrent must be distinct"
    );
}

#[test]
fn marked_rows_are_indicated_and_counted() {
    let mut app = populated();
    let text = render_to_text(&mut app, harness::WIDE);
    assert!(
        !text.contains(crate::ui::theme::GLYPH_MARK),
        "nothing is marked yet"
    );

    app.marked.insert(1);
    app.marked.insert(2);
    let text = render_to_text(&mut app, harness::WIDE);
    assert_eq!(
        text.matches(crate::ui::theme::GLYPH_MARK).count(),
        2,
        "each marked row carries the marker:\n{text}"
    );
    assert!(
        text.contains("2 marked"),
        "the header must say a bulk action is pending:\n{text}"
    );
}

#[test]
fn marks_are_dropped_when_their_torrent_disappears() {
    let mut app = populated();
    app.marked.insert(1);
    app.marked.insert(99);
    // Re-applying a snapshot reconciles marks against what still exists.
    let snapshot = app.snapshot.clone();
    app.update_snapshot(snapshot);
    assert_eq!(app.marked_count(), 1, "the phantom mark must be dropped");
    assert!(app.marked.contains(&1));
}

#[test]
fn help_and_quit_survive_the_narrowest_terminal() {
    // The footer used to drop these silently, leaving a stuck user with no way
    // to discover either.
    let mut app = populated();
    let text = render_to_text(&mut app, harness::MIN);
    let footer = text.lines().last().unwrap_or_default();
    assert!(footer.contains("help"), "footer lost help: {footer:?}");
    assert!(footer.contains("quit"), "footer lost quit: {footer:?}");
}

#[test]
fn a_truncated_footer_says_more_exist() {
    let mut app = populated();
    let text = render_to_text(&mut app, harness::MIN);
    let footer = text.lines().last().unwrap_or_default();
    assert!(
        footer.contains(crate::ui::theme::GLYPH_MORE),
        "truncation must be visible: {footer:?}"
    );
}

#[test]
fn counts_and_rates_do_not_share_glyphs_in_the_header() {
    let mut app = populated();
    let text = render_to_text(&mut app, harness::WIDE);
    let header = text.lines().next().unwrap_or_default();
    // The arrows must appear only as rate markers, so each shows up once.
    let downs = header.matches(crate::ui::theme::GLYPH_DOWN).count();
    let ups = header.matches(crate::ui::theme::GLYPH_UP).count();
    assert_eq!(downs, 1, "download arrow is ambiguous in: {header:?}");
    assert_eq!(ups, 1, "upload arrow is ambiguous in: {header:?}");
    assert!(
        header.contains("downloading") && header.contains("seeding"),
        "counts should be spelled out: {header:?}"
    );
}

#[test]
fn the_header_costs_two_rows_not_three() {
    let mut app = populated();
    let text = render_to_text(&mut app, harness::WIDE);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].contains("kist"), "row 0 is the summary line");
    assert!(
        lines[1].chars().all(|c| c == '─' || c == ' '),
        "row 1 is the rule, got {:?}",
        lines[1]
    );
}

#[test]
fn four_digit_values_are_not_clipped_in_the_list() {
    // The fixture rows are 1000 KiB at 1000 KiB/s, which formats to the widest
    // string each formatter can emit. The list used to show `000.0 KiB`.
    let mut app = populated();
    let text = render_to_text(&mut app, harness::WIDE);
    assert!(
        text.contains("1000.0 KiB"),
        "the size column dropped a digit:\n{text}"
    );
    assert!(
        !text.contains("000.0 KiB\u{0020}") || text.contains("1000.0 KiB"),
        "a clipped size is still present:\n{text}"
    );
    for line in text.lines() {
        assert!(
            !line.contains(" 000.0 "),
            "a value lost its leading digit:\n{line}"
        );
    }
}

#[test]
fn overlays_keep_their_essential_content_at_every_size() {
    for (name, mode, needles) in required_text() {
        for size in sizes() {
            let mut app = populated();
            app.mode = mode;
            let text = render_to_text(&mut app, size);
            for needle in &needles {
                assert!(
                    text.contains(needle),
                    "{name} at {}x{} lost {needle:?}:\n{text}",
                    size.0,
                    size.1
                );
            }
        }
    }
}

#[test]
fn below_minimum_size_shows_only_the_notice() {
    let mut app = populated();
    let text = render_to_text(&mut app, (30, 8));
    assert!(
        text.contains("too small"),
        "expected the minimum-size notice, got:\n{text}"
    );
    for line in text.lines() {
        assert!(crate::format::display_width(line) <= 30);
    }
}

/// Write every mode at every size to `KIST_RENDER_DUMP` for eyeballing.
///
/// Run before and after a visual change to diff the effect:
/// `KIST_RENDER_DUMP=/tmp/before cargo test dump_renders -- --ignored`
#[test]
#[ignore]
fn dump_renders() {
    let Ok(dir) = std::env::var("KIST_RENDER_DUMP") else {
        panic!("set KIST_RENDER_DUMP to a directory");
    };
    std::fs::create_dir_all(&dir).unwrap();
    let mut wrote = 0;

    for (name, build) in modes() {
        for size in sizes() {
            let mut app = build();
            let text = render_to_text(&mut app, size);
            let file = format!("{dir}/{}-{}x{}.txt", name.replace(' ', "-"), size.0, size.1);
            std::fs::write(&file, text).unwrap();
            wrote += 1;
        }
    }
    for (name, tab) in detail_tabs() {
        for size in sizes() {
            let mut app = populated();
            app.mode = Mode::Detail { id: 0 };
            app.detail_tab = tab;
            app.set_detail(Some(detail()));
            let text = render_to_text(&mut app, size);
            let file = format!("{dir}/{}-{}x{}.txt", name.replace(' ', "-"), size.0, size.1);
            std::fs::write(&file, text).unwrap();
            wrote += 1;
        }
    }
    println!("wrote {wrote} renders to {dir}");
}

#[test]
fn minimum_size_still_renders_the_list() {
    let mut app = populated();
    let text = render_to_text(&mut app, harness::MIN);
    assert!(
        !text.contains("too small"),
        "the minimum supported size must render the real layout"
    );
}
