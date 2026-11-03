//! What the pages show: a row per item with its checks and review in words, the
//! filters, an item in full, text from GitHub only ever as text, and the
//! problems said with their fix, on the widgets a tab sends Hangar.

mod common;

use common::*;
use hangar_kit_sdk::api::KitError;
use hangar_kit_sdk::ui::{Element, EmptyKind, Icon, Status};
use serde_json::{Value, json};

fn glyphs(tab: &Tab) -> Vec<(Status, String)> {
    let mut found = Vec::new();
    tab.view().root.for_each(&mut |element| {
        if let Element::Glyph(glyph) = element {
            found.push((glyph.status, glyph.label.clone()));
        }
    });
    found
}

/// The second line of each row, which sits in the row's body.
fn second_lines(tab: &Tab) -> Vec<String> {
    let mut found = Vec::new();
    tab.view().root.for_each(&mut |element| {
        if let Element::List(list) = element {
            for item in &list.items {
                if let Some(Element::Text(text)) = item.body.first() {
                    found.push(plain(&text.text));
                }
            }
        }
    });
    found
}

fn started(id: &str) -> (Tab, Host) {
    let mut host = Host::recorded();
    let tab = Tab::started(id, &mut host);
    (tab, host)
}

#[test]
fn pull_requests_a_row_per_item_with_checks_and_review_in_words_and_a_way_to_hand_it_over() {
    let (mut tab, mut host) = started("pulls");
    let text = tab.text();
    assert!(text.contains("example/widgets"));
    assert!(text.contains("2 checks failing"));
    assert!(text.contains("4 open"));
    let rows = tab.rows();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].0, "Fix the cache key for nested widgets");
    assert!(plain(rows[0].1.as_deref().unwrap()).starts_with("#42 · octocat · fix/cache-key · "));
    let lines = second_lines(&tab);
    assert_eq!(lines[0], "2 checks failing · Review required");
    assert_eq!(lines[1], "Checks passing · Approved");
    assert_eq!(lines[2], "Draft · 1 check failing");
    assert_eq!(lines[3], "No checks · Changes requested");

    let marks = glyphs(&tab);
    assert_eq!(marks[0], (Status::Failure, "2 checks failing".to_owned()));
    assert_eq!(marks[1], (Status::Success, "Checks passing".to_owned()));
    assert_eq!(marks[3], (Status::Neutral, "No checks".to_owned()));

    let menus = tab.menus();
    assert_eq!(menus.len(), 4, "each item has one");
    assert!(
        menus
            .iter()
            .all(|menu| menu.label == "Hand to an agent" && !menu.primary)
    );
    tab.press("Add a dark theme to the settings page");
    tab.settle(&mut host);
    assert_eq!(
        args_of(&host, "pr-view"),
        json!({ "id": "40" }),
        "the row opens its own item"
    );
}

fn args_of(host: &Host, name: &str) -> Value {
    host.runs(name).last().unwrap().params["args"].clone()
}

#[test]
fn a_row_has_its_whole_title_on_hover() {
    let (tab, _) = started("pulls");
    let mut tooltips = Vec::new();
    tab.view().root.for_each(&mut |element| {
        if let Element::List(list) = element {
            tooltips.extend(list.items.iter().map(|item| item.tooltip.clone()));
        }
    });
    assert_eq!(
        tooltips[0].as_deref(),
        Some("Fix the cache key for nested widgets")
    );
}

#[test]
fn in_french_with_its_accents() {
    let mut host = Host::recorded();
    let mut tab = Tab::start_in("pulls", "fr");
    tab.settle(&mut host);
    let text = tab.text();
    assert!(text.contains("2 checks en échec"), "{text}");
    assert!(text.contains("Revue requise"));
    assert!(tab.has_button("Actualiser"));
    assert!(text.contains("4 ouvertes"));
    assert_eq!(tab.menus()[0].label, "Confier à un agent");
    let chips: Vec<String> = tab.filters().into_iter().map(|chip| chip.0).collect();
    assert_eq!(
        chips,
        ["Checks en échec", "À relire", "Masquer les brouillons"]
    );
    assert_eq!(
        tab.search_placeholder().as_deref(),
        Some("Chercher une pull request")
    );
}

#[test]
fn the_filters_chips_that_are_pressed_counts_a_search_that_does_not_lose_a_word() {
    let (mut tab, _) = started("pulls");
    let chips = tab.filters();
    assert_eq!(
        chips,
        [
            ("Failing checks".to_owned(), Some("2".to_owned()), false),
            ("Needs review".to_owned(), Some("1".to_owned()), false),
            ("Hide drafts".to_owned(), None, false),
        ]
    );
    tab.press("Failing checks");
    assert!(tab.filters()[0].2, "pressed");
    assert_eq!(tab.row_numbers(), [42, 39]);
    tab.press("Failing checks");
    assert!(!tab.filters()[0].2);
    tab.type_into("Search a pull request", "cache");
    assert_eq!(
        tab.query().as_deref(),
        Some("cache"),
        "what was typed is what is shown"
    );
    assert_eq!(tab.row_numbers(), [42]);
    // the danger tone of a count that is not zero
    let mut tones = Vec::new();
    tab.view().root.for_each(&mut |element| {
        if let Element::Filter(filter) = element {
            tones.push(filter.count_tone);
        }
    });
    assert_eq!(tones[0], hangar_kit_sdk::ui::Tone::Danger);
    assert_eq!(tones[1], hangar_kit_sdk::ui::Tone::Default);
}

#[test]
fn filters_that_match_nothing_say_so_and_offer_to_clear_them() {
    let (mut tab, _) = started("pulls");
    tab.type_into("Search a pull request", "zzz");
    let text = tab.text();
    assert!(text.contains("Nothing matches these filters"));
    assert!(text.contains("the 4 open"));
    assert_eq!(tab.empties()[0].icon, Some(Icon::Filter));
    tab.press("Clear filters");
    assert_eq!(tab.row_numbers().len(), 4);
    assert_eq!(tab.query().as_deref(), Some(""));
}

#[test]
fn no_item_at_all_is_an_empty_state_that_says_what_fills_it() {
    let mut host = Host::recorded();
    host.command("pr-list", |_| Ok(json!({ "stdout": "[]" })));
    let tab = Tab::started("pulls", &mut host);
    let text = tab.text();
    assert!(text.contains("No open pull request"));
    assert!(text.contains("opened on this repository"));
    assert_eq!(tab.empties()[0].kind, EmptyKind::Empty);
    assert!(text.contains("0 open"));

    let mut host = Host::recorded();
    host.command("issue-list", |_| Ok(json!({ "stdout": "[]" })));
    let tab = Tab::started("issues", &mut host);
    assert!(tab.text().contains("No open issue"));
}

#[test]
fn issues_assignee_and_labels_and_the_unassigned_chip() {
    let (mut tab, _) = started("issues");
    let rows = tab.rows();
    assert_eq!(rows.len(), 2);
    let lines = second_lines(&tab);
    assert_eq!(lines[0], "bug, ui");
    assert_eq!(lines[1], "Assigned to: octocat · enhancement");
    assert_eq!(
        tab.filters(),
        [("Unassigned".to_owned(), Some("1".to_owned()), false)]
    );
    tab.press("Unassigned");
    assert_eq!(tab.row_numbers(), [15]);
    assert_eq!(tab.search_placeholder().as_deref(), Some("Search an issue"));
}

#[test]
fn a_pull_request_in_full_checks_the_review_asked_for_unresolved_comments_the_diff_stat() {
    let (mut tab, mut host) = started("pulls");
    tab.press("Fix the cache key for nested widgets");
    tab.settle(&mut host);
    let text = tab.text();
    assert!(text.contains("#42 Fix the cache key"), "{text}");
    assert_eq!(tab.fact("Author").as_deref(), Some("octocat"));
    assert_eq!(tab.fact("Changes").as_deref(), Some("3 files, +58 −12"));
    assert_eq!(tab.fact("Branch").as_deref(), Some("fix/cache-key → main"));
    assert_eq!(tab.fact("Labels").as_deref(), Some("bug"));
    assert!(text.contains("test (ubuntu)"));
    assert!(text.contains("Unresolved comments"));
    assert!(text.contains("src/cache.js:14"));
    assert!(!text.contains("nit: rename"));
    assert!(text.contains("https://github.com/example/widgets/pull/42"));
    assert!(text.contains("2 checks failing"));
    assert!(text.contains("Review required"));
    assert_eq!(
        tab.primaries(),
        ["Hand to an agent"],
        "one primary action: hand it over"
    );
    // the failing checks first
    let marks = glyphs(&tab);
    assert_eq!(marks[0].0, Status::Failure);
    assert_eq!(marks[1].0, Status::Failure);
    assert_eq!(marks[2].0, Status::Running);
    assert_eq!(marks[3].0, Status::Success);
    // and the files, the reviews
    assert!(text.contains("src/cache.js"));
    let files: Vec<(String, Option<String>)> = tab
        .rows()
        .into_iter()
        .filter(|(_, sub)| sub.as_deref().is_some_and(|sub| sub.starts_with('+')))
        .collect();
    assert_eq!(
        files,
        [
            ("src/cache.js".to_owned(), Some("+30 −10".to_owned())),
            ("src/widget.js".to_owned(), Some("+20 −2".to_owned())),
            ("test/cache.test.js".to_owned(), Some("+8 −0".to_owned())),
        ]
    );
    assert!(text.contains("hubot · Changes requested"));
    assert!(text.contains("The key must include the parent id."));
    tab.press("Back");
    tab.settle(&mut host);
    assert_eq!(tab.row_numbers().len(), 4);
}

#[test]
fn an_issue_in_full_its_facts_its_description_and_its_latest_comments() {
    let (mut tab, mut host) = started("issues");
    tab.press("The sidebar flickers on resize");
    tab.settle(&mut host);
    let text = tab.text();
    assert!(text.contains("#15 The sidebar flickers on resize"));
    assert!(text.contains("open"));
    assert_eq!(tab.fact("Author").as_deref(), Some("octocat"));
    assert_eq!(tab.fact("Assigned to"), None, "nobody: no line for it");
    assert_eq!(tab.fact("Labels").as_deref(), Some("bug, ui"));
    assert!(text.contains("When I drag the window edge the sidebar flickers."));
    assert!(text.contains("I can reproduce on 1.8."));
    assert_eq!(tab.primaries(), ["Hand to an agent"]);
    assert!(tab.has_button("Copy link"));
}

#[test]
fn the_description_is_the_raw_text_and_shows_a_cut_one_with_an_ellipsis() {
    let mut host = Host::recorded();
    let mut view: Value = json_fixture("pr-view-failing.json");
    view["body"] = json!(format!("{}tail", "x".repeat(4100)));
    host.command("pr-view", move |_| {
        Ok(json!({ "stdout": view.to_string() }))
    });
    let mut tab = Tab::started("pulls", &mut host);
    tab.press("Fix the cache key for nested widgets");
    tab.settle(&mut host);
    let mut shown = String::new();
    tab.view().root.for_each(&mut |element| {
        if let Element::Pre(pre) = element {
            shown = pre.text.clone();
        }
    });
    assert_eq!(shown.chars().count(), 4001);
    assert!(shown.ends_with('…'));
    assert!(!shown.contains("tail"));
}

#[test]
fn text_from_github_is_only_ever_text_never_markup() {
    let mut hostile = json_fixture("pr-view-failing.json");
    hostile["title"] = json!("<img src=x onerror=alert(1)> title");
    hostile["body"] = json!("<script>alert(1)</script>");
    let mut host = Host::recorded();
    host.command("pr-view", move |_| {
        Ok(json!({ "stdout": hostile.to_string() }))
    });
    let mut tab = Tab::started("pulls", &mut host);
    tab.press("Fix the cache key for nested widgets");
    tab.settle(&mut host);
    let text = tab.text();
    assert!(text.contains("<img src=x onerror=alert(1)> title"));
    assert!(text.contains("<script>alert(1)</script>"));
    // and in the page it is a text of the title and a pre, nothing else
    let mut pres = Vec::new();
    tab.view().root.for_each(&mut |element| {
        if let Element::Pre(pre) = element {
            pres.push(pre.text.clone());
        }
    });
    assert_eq!(pres, ["<script>alert(1)</script>"]);
}

#[test]
fn problems_are_said_with_what_to_do_and_the_command_that_fixes_them() {
    let mut host = Host::recorded();
    host.command("repo-info", |_| Err(KitError::new("not_found", "x")));
    let tab = Tab::started("pulls", &mut host);
    let text = tab.text();
    assert!(text.contains("GitHub CLI (gh) is not installed"));
    assert!(text.contains("cli.github.com"));
    let empty = &tab.empties()[0];
    assert_eq!(empty.kind, EmptyKind::Error);
    let mut code = Vec::new();
    for child in &empty.children {
        child.for_each(&mut |element| {
            if let Element::Text(text) = element
                && text.code
            {
                code.push(text.text.clone());
            }
        });
    }
    assert_eq!(code, ["gh auth login"]);
    assert!(tab.has_button("Try again"));
    assert_eq!(tab.primaries(), ["Try again"]);

    let mut host = Host::recorded();
    host.command("repo-info", |_| failed("not-logged-in.stderr", 4));
    let tab = Tab::started("issues", &mut host);
    assert!(tab.text().contains("gh is not signed in to GitHub"));
    assert!(tab.has_button("Try again"));
    assert!(tab.has_button("Copy the command"));

    let mut host = Host::recorded();
    host.command("repo-info", |_| failed("not-a-git-repo.stderr", 1));
    let mut tab = Tab::start_in("pulls", "fr");
    tab.settle(&mut host);
    let text = tab.text();
    assert!(text.contains("Ce dossier n’est pas un dépôt GitHub"));
    assert!(
        text.contains("not a git repository"),
        "gh's own words are shown too"
    );
    let mut pres = 0;
    tab.view().root.for_each(&mut |element| {
        pres += matches!(element, Element::Pre(_)) as usize;
    });
    assert_eq!(pres, 1);

    let mut host = Host::bare();
    let tab = Tab::started("pulls", &mut host);
    assert!(tab.text().contains("may not run this command"));
    assert!(
        !tab.has_button("Try again"),
        "retrying a refusal does nothing"
    );
    assert_eq!(tab.empties()[0].kind, EmptyKind::Denied);
}

#[test]
fn an_error_without_a_known_kind_shows_what_gh_said_without_its_colours() {
    let mut host = Host::recorded();
    host.command("pr-list", |_| {
        Ok(json!({ "exitCode": 1, "stderr": "\u{1b}[31mfatal:\u{1b}[0m the sky fell\n" }))
    });
    let tab = Tab::started("pulls", &mut host);
    assert!(tab.text().contains("GitHub did not answer"));
    assert!(tab.text().contains("fatal: the sky fell"));
    assert!(!tab.text().contains('\u{1b}'));
}

#[test]
fn every_state_is_drawn_loading_empty_failed_denied() {
    let tab = Tab::start("pulls");
    assert_eq!(tab.empties()[0].kind, EmptyKind::Loading);
    assert!(tab.text().contains("Reading from GitHub…"));

    let mut host = Host::recorded();
    host.command("pr-list", |_| Ok(json!({ "stdout": "[]" })));
    assert_eq!(
        Tab::started("pulls", &mut host).empties()[0].kind,
        EmptyKind::Empty
    );

    let mut host = Host::recorded();
    host.command("repo-info", |_| failed("not-logged-in.stderr", 4));
    assert_eq!(
        Tab::started("pulls", &mut host).empties()[0].kind,
        EmptyKind::Error
    );

    assert_eq!(
        Tab::started("pulls", &mut Host::bare()).empties()[0].kind,
        EmptyKind::Denied
    );
}

#[test]
fn the_view_holds_no_colour_of_its_own_and_loads_nothing_from_outside() {
    for (name, source) in [
        ("view.rs", include_str!("../src/view.rs")),
        ("lib.rs", include_str!("../src/lib.rs")),
    ] {
        assert!(
            !source.contains("Ink::"),
            "{name}: an ink is a picture's colour"
        );
        assert!(!source.contains("rgb"), "{name}: no colour");
        assert!(!source.contains("http://"), "{name}: no external address");
        assert!(!source.contains("https://"), "{name}: no external address");
        // a hex colour: # and six or three hex digits, not inside a format string like "#{}"
        let hex = source.match_indices('#').any(|(at, _)| {
            let digits: String = source[at + 1..]
                .chars()
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            matches!(digits.len(), 3 | 6 | 8)
                && digits.chars().any(|c| c.is_ascii_digit())
                && !source[..at].ends_with("&")
        });
        assert!(!hex, "{name}: no colour");
    }
}

#[test]
fn a_page_keeps_one_primary_action() {
    let (mut tab, mut host) = started("pulls");
    assert!(tab.primaries().is_empty(), "a list has none");
    tab.press("Fix the cache key for nested widgets");
    assert!(tab.primaries().is_empty(), "reading has none");
    tab.settle(&mut host);
    assert_eq!(tab.primaries().len(), 1);
}

#[test]
fn no_string_key_reaches_the_page() {
    use kit_github_tab::strings::EN;
    let keys: Vec<&str> = EN.iter().map(|(key, _)| *key).collect();
    for locale in ["en", "fr"] {
        for id in ["pulls", "issues"] {
            let search = match (locale, id) {
                ("en", "pulls") => "Search a pull request",
                ("en", _) => "Search an issue",
                (_, "pulls") => "Chercher une pull request",
                _ => "Chercher une issue",
            };
            let first = if id == "pulls" {
                "Fix the cache key for nested widgets"
            } else {
                "The sidebar flickers on resize"
            };
            let mut host = Host::recorded();
            let mut tab = Tab::start_in(id, locale);
            tab.settle(&mut host);
            let mut pages = vec![tab.text()];
            tab.type_into(search, "zzz");
            pages.push(tab.text());
            tab.type_into(search, "");
            tab.open_hand(
                tab.menus()[0]
                    .id
                    .trim_start_matches("hand-")
                    .parse()
                    .unwrap(),
                &mut host,
            );
            pages.push(tab.text());
            tab.press(first);
            pages.push(tab.text());
            tab.settle(&mut host);
            pages.push(tab.text());
            for page in pages {
                for line in page.lines() {
                    assert!(
                        !keys.contains(&line.trim()),
                        "{locale} {id}: the key {line} is on a page"
                    );
                }
            }
        }
    }
}
