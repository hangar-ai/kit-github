//! The tabs' behaviour against a Hangar that answers like the real one and a
//! clock the test sets: what they ask, when they refresh, what the badge says,
//! what is saved, and what an agent receives. test/controller.test.js, case
//! for case; every call is also held to the permissions the manifest gives
//! the tab (`Tab::settle`).

mod common;

use common::*;
use hangar_kit_sdk::api::KitError;
use kit_github_tab::{MIN_GAP_MS, REFRESH, SAVE_DELAY};
use serde_json::{Value, json};

const MINUTE: u32 = 60_000;

fn args(call: &hangar_kit_sdk::ui::abi::Call) -> Value {
    call.params["args"].clone()
}

#[test]
fn pull_requests_the_list_the_repository_and_a_badge_for_the_failing_checks() {
    let mut host = Host::recorded();
    let tab = Tab::started("pulls", &mut host);
    assert_eq!(tab.row_numbers(), [42, 40, 39, 37]);
    assert!(tab.text().contains("example/widgets"));
    assert_eq!(
        host.badges(),
        [json!({ "state": "failure", "text": "2 fail" })]
    );
    assert_eq!(host.runs("pr-list").len(), 1);
    assert_eq!(
        host.runs("issue-list").len(),
        0,
        "the pull requests tab never reads issues"
    );
}

#[test]
fn issues_the_list_and_no_badge() {
    let mut host = Host::recorded();
    let tab = Tab::started("issues", &mut host);
    assert_eq!(tab.row_numbers(), [15, 11]);
    assert_eq!(host.badges(), [json!({ "state": null })]);
    assert_eq!(host.runs("pr-list").len(), 0);
}

#[test]
fn the_saved_filters_are_read_before_the_list_is_asked_for() {
    let mut tab = Tab::start("pulls");
    assert_eq!(
        tab.methods(),
        ["data.read"],
        "nothing else until the filters came"
    );
    let read = tab.reply_to("data.read", Err(KitError::new("not_found", "none")));
    assert_eq!(read.params, json!({ "path": "filters-pulls.json" }));
    assert_eq!(tab.methods(), ["command.run"]);
    let mut host = Host::recorded();
    tab.settle(&mut host);
    assert_eq!(
        tab.row_numbers().len(),
        4,
        "none saved: the defaults, every row"
    );

    let issues = Tab::start("issues");
    assert_eq!(
        issues.calls()[0].params,
        json!({ "path": "filters-issues.json" })
    );
}

#[test]
fn it_refreshes_every_60_s_while_visible_and_not_while_hidden() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("pulls", &mut host);
    assert_eq!(tab.refresh_every(), Some(MINUTE));
    assert_eq!(REFRESH.as_millis() as u32, MINUTE);
    assert_eq!(host.runs("pr-list").len(), 1);

    tab.timer("refresh");
    tab.settle(&mut host);
    assert_eq!(host.runs("pr-list").len(), 2);
    assert_eq!(
        host.runs("repo-info").len(),
        1,
        "the repository is read once"
    );
    assert_eq!(
        host.badges().len(),
        1,
        "an unchanged badge is not set again"
    );

    tab.visible(false);
    assert_eq!(tab.refresh_every(), None, "hidden: nothing is asked for");
    assert!(tab.subscriptions().timers.is_empty());

    tab.set_now("2026-10-31T17:05:00Z");
    tab.visible(true);
    tab.settle(&mut host);
    assert_eq!(host.runs("pr-list").len(), 3, "back in view: at once");
    assert_eq!(tab.refresh_every(), Some(MINUTE));
}

#[test]
fn back_in_view_soon_after_a_read_does_not_read_again() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("pulls", &mut host);
    assert_eq!(MIN_GAP_MS, 5000);
    tab.set_now("2026-10-31T17:02:04Z");
    tab.visible(false);
    tab.visible(true);
    tab.settle(&mut host);
    assert_eq!(host.runs("pr-list").len(), 1, "4 s since the last read");
    tab.visible(false);
    tab.set_now("2026-10-31T17:02:06Z");
    tab.visible(true);
    tab.settle(&mut host);
    assert_eq!(host.runs("pr-list").len(), 2, "6 s since it");
}

#[test]
fn a_tab_started_behind_another_reads_once_and_polls_when_it_is_seen() {
    let mut host = Host::recorded();
    let mut tab = Tab::start_hidden("pulls");
    tab.settle(&mut host);
    assert_eq!(tab.refresh_every(), None);
    assert_eq!(
        tab.row_numbers().len(),
        4,
        "it reads once, to have something to show"
    );
    tab.set_now("2026-10-31T17:09:00Z");
    tab.visible(true);
    assert_eq!(tab.refresh_every(), Some(MINUTE));
    tab.settle(&mut host);
    assert_eq!(host.runs("pr-list").len(), 2);
}

#[test]
fn a_refresh_on_demand_and_a_wrong_answer_keeps_the_last_list() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("pulls", &mut host);
    host.command("pr-list", |_| ok("repo-info.json")); // not a list at all
    tab.press("Refresh");
    assert!(
        tab.has_button("Refreshing…"),
        "the button says so while it reads"
    );
    tab.settle(&mut host);
    assert!(
        tab.text()
            .contains("Could not refresh. The last list stays shown.")
    );
    assert_eq!(tab.row_numbers().len(), 4);
    host.command("pr-list", |_| ok("pr-list.json"));
    tab.press("Refresh");
    tab.settle(&mut host);
    assert!(!tab.text().contains("Could not refresh"));
    assert_eq!(host.runs("pr-list").len(), 3);
}

#[test]
fn gh_missing_not_signed_in_no_repository_each_is_shown_and_asked_again_each_minute() {
    type Case = (fn() -> Answer, &'static str);
    let cases: [Case; 4] = [
        (
            || Err(KitError::new("not_found", "gh is not installed")),
            "GitHub CLI (gh) is not installed",
        ),
        (
            || failed("not-logged-in.stderr", 4),
            "gh is not signed in to GitHub",
        ),
        (
            || failed("no-repo.stderr", 1),
            "This folder is not a GitHub repository",
        ),
        (
            || failed("not-a-git-repo.stderr", 1),
            "This folder is not a GitHub repository",
        ),
    ];
    for (answer, title) in cases {
        let mut host = Host::recorded();
        host.command("repo-info", move |_| answer());
        let mut tab = Tab::started("pulls", &mut host);
        assert!(tab.text().contains(title), "{title}: {}", tab.text());
        assert_eq!(host.badges(), [json!({ "state": null })]);
        tab.timer("refresh");
        tab.settle(&mut host);
        tab.timer("refresh");
        tab.settle(&mut host);
        assert_eq!(
            host.runs("repo-info").len(),
            3,
            "it asks again each minute, in case the person fixed it"
        );
        assert!(tab.text().contains(title));
    }
}

#[test]
fn denied_degrades_to_a_designed_state_no_command_no_panic() {
    let mut host = Host::bare();
    let tab = Tab::started("pulls", &mut host);
    assert!(tab.text().contains("This tab may not run this command"));
    assert!(
        !tab.has_button("Try again"),
        "retrying a refusal does nothing"
    );
    assert_eq!(tab.empties()[0].kind, hangar_kit_sdk::ui::EmptyKind::Denied);
}

#[test]
fn a_blip_after_a_good_list_keeps_the_list_and_says_so() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("pulls", &mut host);
    host.command("pr-list", |_| {
        Ok(json!({ "exitCode": null, "timedOut": true }))
    });
    tab.press("Refresh");
    tab.settle(&mut host);
    assert_eq!(tab.row_numbers().len(), 4);
    assert!(tab.text().contains("Could not refresh"));
}

#[test]
fn a_retry_that_fails_after_a_fatal_error_is_drawn_not_hidden_behind_a_loading_screen() {
    // The page kept the old rows and stayed on "Reading from GitHub…" when a
    // retry failed with an error that was not fatal.
    let mut host = Host::recorded();
    let mut tab = Tab::started("pulls", &mut host);
    host.command("pr-list", |_| failed("not-logged-in.stderr", 4));
    tab.press("Refresh");
    tab.settle(&mut host);
    assert!(tab.text().contains("gh is not signed in to GitHub"));
    host.command("pr-list", |_| {
        Ok(json!({ "exitCode": null, "timedOut": true }))
    });
    tab.press("Try again");
    assert!(tab.text().contains("Reading from GitHub…"));
    tab.settle(&mut host);
    assert!(tab.text().contains("gh did not answer in time"));
    host.command("pr-list", |_| ok("pr-list.json"));
    tab.press("Try again");
    tab.settle(&mut host);
    assert_eq!(tab.row_numbers().len(), 4);
}

#[test]
fn filters_are_read_at_the_start_and_saved_after_a_pause_not_at_each_key() {
    let saved = r#"{"query":"cache","failing":true,"review":false,"hideDrafts":false}"#;
    let mut host = Host::recorded().with_data("filters-pulls.json", saved);
    let mut tab = Tab::started("pulls", &mut host);
    assert_eq!(tab.query().as_deref(), Some("cache"));
    assert!(tab.filters()[0].2, "failing checks is on");
    assert_eq!(tab.row_numbers(), [42], "and filters");

    tab.type_into("Search a pull request", "c");
    tab.type_into("Search a pull request", "ca");
    tab.press("Needs review");
    tab.settle(&mut host);
    assert_eq!(host.calls_to("data.write").len(), 0);
    assert!(tab.waits("save"));
    let wait = tab
        .subscriptions()
        .timers
        .iter()
        .find(|timer| timer.id.starts_with("~after:save:"))
        .map(|timer| timer.every_ms);
    assert_eq!(wait, Some(SAVE_DELAY.as_millis() as u32), "700 ms");
    tab.elapse("save");
    tab.settle(&mut host);
    assert_eq!(
        host.calls_to("data.write").len(),
        1,
        "one write for the three changes"
    );
    let written: Value = serde_json::from_str(&host.files["filters-pulls.json"]).unwrap();
    assert_eq!(
        written,
        json!({ "query": "ca", "failing": true, "review": true, "hideDrafts": false })
    );
    assert!(!tab.waits("save"));

    tab.type_into("Search a pull request", "zzz");
    tab.press("Clear filters");
    tab.elapse("save");
    tab.settle(&mut host);
    let cleared: Value = serde_json::from_str(&host.files["filters-pulls.json"]).unwrap();
    assert_eq!(
        cleared,
        json!({ "query": "", "failing": false, "review": false, "hideDrafts": false })
    );
    assert_eq!(tab.row_numbers().len(), 4);
}

#[test]
fn filters_the_issues_tab_keeps_its_own_file_and_a_refused_data_folder_changes_nothing() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("issues", &mut host);
    tab.press("Unassigned");
    tab.elapse("save");
    tab.settle(&mut host);
    assert!(host.files.contains_key("filters-issues.json"));
    assert!(!host.files.contains_key("filters-pulls.json"));
    assert_eq!(tab.row_numbers(), [15]);

    let mut denied = Host::recorded();
    denied.deny_data = true;
    let mut tab = Tab::started("pulls", &mut denied);
    tab.press("Failing checks");
    tab.elapse("save");
    tab.settle(&mut denied);
    assert!(
        tab.filters()[0].2,
        "it still filters, it only cannot remember"
    );
    assert_eq!(tab.row_numbers(), [42, 39]);
    assert!(denied.files.is_empty());
}

#[test]
fn the_search_field_clears_with_its_button() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("pulls", &mut host);
    tab.type_into("Search a pull request", "cache");
    assert_eq!(tab.row_numbers(), [42]);
    let clear = {
        let mut found = None;
        tab.view().root.for_each(&mut |element| {
            if let hangar_kit_sdk::ui::Element::Search(search) = element {
                found = search.on_clear;
            }
        });
        found.expect("a search with text can be cleared")
    };
    tab.ui(clear, None);
    assert_eq!(tab.query().as_deref(), Some(""));
    assert_eq!(tab.row_numbers().len(), 4);
}

#[test]
fn a_pull_request_in_full_the_view_and_its_threads_asked_with_the_repositorys_own_owner_and_name() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("pulls", &mut host);
    tab.press("Fix the cache key for nested widgets");
    assert!(tab.text().contains("Reading from GitHub…"));
    assert_eq!(
        tab.methods().len(),
        2,
        "the view and the threads, both at once"
    );
    tab.settle(&mut host);
    let text = tab.text();
    assert!(text.contains("#42 Fix the cache key for nested widgets"));
    assert!(text.contains("src/cache.js:14"));
    assert_eq!(args(host.runs("pr-view")[0]), json!({ "id": "42" }));
    assert_eq!(
        args(host.runs("pr-threads")[0]),
        json!({ "id": "42", "owner": "example", "name": "widgets" })
    );

    tab.press("Back");
    tab.settle(&mut host);
    assert_eq!(tab.row_numbers().len(), 4);
    assert_eq!(host.runs("pr-list").len(), 1, "just refreshed: not again");
}

#[test]
fn back_to_a_list_read_long_ago_reads_it_again() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("pulls", &mut host);
    tab.press("Fix the cache key for nested widgets");
    tab.settle(&mut host);
    tab.set_now("2026-10-31T17:03:00Z");
    tab.press("Back");
    tab.settle(&mut host);
    assert_eq!(host.runs("pr-list").len(), 2);
}

#[test]
fn review_threads_that_cannot_be_read_do_not_hide_the_pull_request() {
    let mut host = Host::recorded();
    host.command("pr-threads", |_| {
        Ok(json!({ "exitCode": 1, "stderr": "boom" }))
    });
    let mut tab = Tab::started("pulls", &mut host);
    tab.press("Fix the cache key for nested widgets");
    tab.settle(&mut host);
    assert!(tab.text().contains("#42 Fix the cache key"));
    assert!(
        tab.text()
            .contains("The review comments could not be read.")
    );
}

#[test]
fn a_pull_request_that_cannot_be_read_says_so_and_tries_again_for_itself() {
    let mut host = Host::recorded();
    host.command("pr-view", |_| {
        Ok(json!({ "exitCode": 1, "stderr": "boom" }))
    });
    let mut tab = Tab::started("pulls", &mut host);
    tab.press("Fix the cache key for nested widgets");
    tab.settle(&mut host);
    assert!(tab.text().contains("GitHub did not answer"));
    assert!(tab.text().contains("boom"));
    host.command("pr-view", |_| ok("pr-view-failing.json"));
    tab.press("Try again");
    tab.settle(&mut host);
    assert!(tab.text().contains("#42 Fix the cache key"));
    assert_eq!(
        host.runs("pr-list").len(),
        1,
        "it read the item again, not the list"
    );
}

#[test]
fn a_refresh_while_a_detail_is_open_does_not_close_it() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("pulls", &mut host);
    tab.press("Fix the cache key for nested widgets");
    tab.settle(&mut host);
    tab.timer("refresh");
    tab.settle(&mut host);
    assert_eq!(host.runs("pr-list").len(), 2);
    assert!(tab.text().contains("#42 Fix the cache key"));
    assert!(tab.has_button("Back"));
    // nor does a failing one
    host.command("pr-list", |_| {
        Ok(json!({ "exitCode": null, "timedOut": true }))
    });
    tab.timer("refresh");
    tab.settle(&mut host);
    assert!(tab.text().contains("#42 Fix the cache key"));
}

#[test]
fn a_detail_that_is_answered_after_the_person_went_back_is_dropped() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("pulls", &mut host);
    tab.press("Fix the cache key for nested widgets");
    tab.press("Back");
    tab.settle(&mut host); // answers for the item come now
    assert_eq!(tab.row_numbers().len(), 4, "the list stays");
    assert!(!tab.has_button("Back"));
}

#[test]
fn panes_only_the_claude_code_agents_that_can_receive() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("pulls", &mut host);
    let before = tab.menus();
    assert_eq!(before.len(), 4, "one in each row");
    assert!(before[0].lines[0].3, "nothing to pick before it opens");

    tab.open_hand(42, &mut host);
    let menu = tab
        .menus()
        .into_iter()
        .find(|menu| menu.id == "hand-42")
        .unwrap();
    assert_eq!(
        menu.lines,
        [
            ("Ava".to_owned(), "Ava".to_owned(), None, false),
            (
                "Max".to_owned(),
                "Max".to_owned(),
                Some("working".to_owned()),
                false
            ),
        ]
    );
    let other = tab
        .menus()
        .into_iter()
        .find(|menu| menu.id == "hand-40")
        .unwrap();
    assert!(
        other.lines[0].3,
        "another row's menu waits for its own opening"
    );

    let mut denied = Host::recorded();
    denied.deny_panes = true;
    let mut tab = Tab::started("pulls", &mut denied);
    tab.open_hand(42, &mut denied);
    assert_eq!(denied.calls_to("ui.toast").len(), 1);
    assert!(
        denied.calls_to("ui.toast")[0].params["message"]
            .as_str()
            .unwrap()
            .contains("Grant panes.read")
    );
    assert_eq!(denied.calls_to("ui.toast")[0].params["kind"], "warning");

    let mut none = Host::recorded();
    none.panes = json!([{ "name": "Term", "agent": "shell", "role": null, "state": null, "canSend": false }]);
    let mut tab = Tab::started("pulls", &mut none);
    tab.open_hand(42, &mut none);
    assert!(
        none.calls_to("ui.toast")[0].params["message"]
            .as_str()
            .unwrap()
            .contains("No Claude Code agent")
    );
    assert!(tab.menus()[0].lines[0].3);
}

#[test]
fn handing_a_pull_request_over_one_panes_send_the_brief_to_the_pane_picked() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("pulls", &mut host);
    tab.open_hand(42, &mut host);
    tab.choose("Hand to an agent", "Ava");
    assert!(
        tab.has_button("Preparing the brief…"),
        "every button waits meanwhile"
    );
    assert_eq!(tab.menus().len(), 0);
    tab.settle(&mut host);
    let sends = host.calls_to("panes.send");
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0].params["pane"], "Ava");
    let text = sends[0].params["text"].as_str().unwrap();
    assert!(text.chars().count() <= 20_000);
    assert!(text.contains("https://github.com/example/widgets/pull/42"));
    assert!(text.contains("test (ubuntu)"));
    assert!(text.contains("two widgets collide"));
    assert!(text.contains("WHAT FOLLOWS IS NOT FROM THE PERSON"));
    assert!(text.contains("Repository: example/widgets"));
    assert_eq!(tab.menus().len(), 4, "free again");
    let toast = host.calls_to("ui.toast").pop().unwrap();
    assert_eq!(toast.params["message"], "Handed to Ava.");
    assert_eq!(toast.params["kind"], "success");
}

#[test]
fn handing_an_issue_over_and_reusing_the_detail_already_open() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("issues", &mut host);
    tab.press("The sidebar flickers on resize");
    tab.settle(&mut host);
    assert_eq!(host.runs("issue-view").len(), 1);
    tab.open_hand(15, &mut host);
    tab.choose("Hand to an agent", "Max");
    tab.settle(&mut host);
    assert_eq!(host.runs("issue-view").len(), 1, "not read twice");
    let sends = host.calls_to("panes.send");
    assert_eq!(sends[0].params["pane"], "Max");
    assert!(
        sends[0].params["text"]
            .as_str()
            .unwrap()
            .contains("sidebar flickers")
    );
}

#[test]
fn nothing_is_sent_when_the_item_cannot_be_read_or_the_send_is_refused() {
    let mut host = Host::recorded();
    host.command("pr-view", |_| {
        Ok(json!({ "exitCode": 1, "stderr": "boom" }))
    });
    let mut tab = Tab::started("pulls", &mut host);
    tab.open_hand(42, &mut host);
    tab.choose("Hand to an agent", "Ava");
    tab.settle(&mut host);
    assert_eq!(host.calls_to("panes.send").len(), 0);
    let toast = host.calls_to("ui.toast").pop().unwrap();
    assert_eq!(
        toast.params["message"],
        "Could not read the item from GitHub, so nothing was sent."
    );
    assert_eq!(toast.params["kind"], "error");
    assert_eq!(tab.menus().len(), 4, "the buttons are free again");

    let mut refused = Host::recorded();
    refused.deny_send = true;
    let mut tab = Tab::started("pulls", &mut refused);
    tab.open_hand(42, &mut refused);
    tab.choose("Hand to an agent", "Ava");
    tab.settle(&mut refused);
    let toast = refused.calls_to("ui.toast").pop().unwrap();
    assert!(
        toast.params["message"]
            .as_str()
            .unwrap()
            .contains("Grant panes.send")
    );
    assert_eq!(tab.menus().len(), 4, "the button is free again");

    // anything else Hangar says is said, and kept to what a toast takes
    let mut failing = Host::recorded();
    let mut tab = Tab::started("pulls", &mut failing);
    tab.open_hand(42, &mut failing);
    tab.choose("Hand to an agent", "Ava");
    for _ in 0..2 {
        tab.reply_to("command.run", complete(ok("pr-view-failing.json")));
    }
    tab.reply_to("panes.send", Err(KitError::new("failed", "m".repeat(900))));
    let toast = tab
        .calls()
        .iter()
        .find(|call| call.method == "ui.toast")
        .unwrap()
        .clone();
    let message = toast.params["message"].as_str().unwrap();
    assert!(message.starts_with("Could not hand it over: mmm"));
    assert!(message.chars().count() <= 500);
}

#[test]
fn nothing_it_runs_can_write_to_github_every_command_it_calls_is_read_only() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("pulls", &mut host);
    tab.press("Fix the cache key for nested widgets");
    tab.settle(&mut host);
    tab.open_hand(42, &mut host);
    tab.choose("Hand to an agent", "Ava");
    tab.settle(&mut host);
    let mut names: Vec<String> = host
        .calls_to("command.run")
        .iter()
        .map(|call| call.params["name"].as_str().unwrap().to_owned())
        .collect();
    names.sort();
    names.dedup();
    assert_eq!(names, ["pr-list", "pr-threads", "pr-view", "repo-info"]);

    let mut host = Host::recorded();
    let mut tab = Tab::started("issues", &mut host);
    tab.press("The sidebar flickers on resize");
    tab.settle(&mut host);
    let mut names: Vec<String> = host
        .calls_to("command.run")
        .iter()
        .map(|call| call.params["name"].as_str().unwrap().to_owned())
        .collect();
    names.sort();
    names.dedup();
    assert_eq!(names, ["issue-list", "issue-view", "repo-info"]);
}

#[test]
fn every_method_it_calls_is_one_the_manifest_grants_that_tab() {
    // Tab::settle holds each call to the tab's permissions, with the gate's own
    // rules (kit-sdk's api::may_call); this goes through everything a person can do.
    for id in ["pulls", "issues"] {
        let mut host = Host::recorded();
        let mut tab = Tab::started(id, &mut host);
        tab.press("Refresh");
        tab.settle(&mut host);
        tab.type_into(
            if id == "pulls" {
                "Search a pull request"
            } else {
                "Search an issue"
            },
            "x",
        );
        tab.elapse("save");
        tab.settle(&mut host);
        tab.type_into(
            if id == "pulls" {
                "Search a pull request"
            } else {
                "Search an issue"
            },
            "",
        );
        let first = tab.rows()[0].0.clone();
        tab.press(&first);
        tab.settle(&mut host);
        tab.open_hand(
            tab.menus()[0]
                .id
                .trim_start_matches("hand-")
                .parse()
                .unwrap(),
            &mut host,
        );
        tab.choose("Hand to an agent", "Ava");
        tab.settle(&mut host);
        tab.press("Copy link");
        tab.settle(&mut host);
        let seen: std::collections::BTreeSet<&str> =
            host.log.iter().map(|call| call.method.as_str()).collect();
        for method in [
            "command.run",
            "data.read",
            "data.write",
            "panes.list",
            "panes.send",
            "tab.setBadge",
            "ui.copy",
            "ui.toast",
        ] {
            assert!(
                seen.contains(method),
                "{id} never called {method}: {seen:?}"
            );
        }
    }
}

#[test]
fn the_language_follows_hangars_and_the_badge_is_said_again_in_it() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("pulls", &mut host);
    assert!(tab.has_button("Refresh"));
    tab.event("theme", json!({ "scheme": "dark", "locale": "fr" }));
    tab.settle(&mut host);
    assert!(tab.has_button("Actualiser"));
    assert!(tab.text().contains("4 ouvertes"));
    assert_eq!(
        host.badges(),
        [
            json!({ "state": "failure", "text": "2 fail" }),
            json!({ "state": "failure", "text": "2 échec" }),
        ]
    );
    // the same language again, or another that is read as English? nothing to say
    tab.event("theme", json!({ "scheme": "light", "locale": "fr-CA" }));
    tab.settle(&mut host);
    assert_eq!(host.badges().len(), 2);
    tab.event("theme", json!({ "scheme": "light", "locale": "en" }));
    tab.settle(&mut host);
    assert_eq!(host.badges().len(), 3);
    assert!(tab.has_button("Refresh"));
}

#[test]
fn a_tab_started_in_french_speaks_french() {
    let mut host = Host::recorded();
    let mut tab = Tab::start_in("issues", "fr");
    tab.settle(&mut host);
    assert!(tab.text().contains("2 ouvertes"));
    assert!(tab.has_button("Actualiser"));
}

#[test]
fn copying_a_link_says_so_once_hangar_has_done_it() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("pulls", &mut host);
    tab.press("Fix the cache key for nested widgets");
    tab.settle(&mut host);
    tab.press("Copy link");
    let copy = tab.calls()[0].clone();
    assert_eq!(copy.method, "ui.copy");
    assert_eq!(
        copy.params,
        json!({ "text": "https://github.com/example/widgets/pull/42" })
    );
    assert!(
        host.calls_to("ui.toast").is_empty(),
        "not before it is done"
    );
    tab.settle(&mut host);
    assert_eq!(
        host.calls_to("ui.toast")[0].params["message"],
        "Link copied."
    );
}

#[test]
fn a_copy_hangar_refuses_does_not_say_it_happened() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("pulls", &mut host);
    tab.press("Fix the cache key for nested widgets");
    tab.settle(&mut host);
    tab.press("Copy link");
    tab.reply_to(
        "ui.copy",
        Err(KitError::new("denied", "a tab may not fill the clipboard")),
    );
    assert!(tab.calls().is_empty(), "no toast is asked for");
}

#[test]
fn the_command_that_fixes_gh_can_be_copied() {
    let mut host = Host::recorded();
    host.command("repo-info", |_| failed("not-logged-in.stderr", 4));
    let mut tab = Tab::started("pulls", &mut host);
    tab.press("Copy the command");
    assert_eq!(tab.calls()[0].params, json!({ "text": "gh auth login" }));
    tab.settle(&mut host);
    assert_eq!(
        host.calls_to("ui.toast")[0].params["message"],
        "Command copied."
    );
}

#[test]
fn a_late_answer_to_an_earlier_refresh_does_not_overwrite_a_later_one() {
    let mut host = Host::recorded();
    let mut tab = Tab::started("pulls", &mut host);
    tab.press("Refresh");
    let first = tab.calls()[0].id;
    tab.timer("refresh"); // a second read starts before the first is answered
    let second = tab.calls()[1].id;
    tab.reply(second, ok("pr-list.json"));
    tab.reply(first, Ok(json!({ "exitCode": 0, "stdout": "[]" })));
    assert_eq!(tab.row_numbers().len(), 4, "the earlier answer is dropped");
}
