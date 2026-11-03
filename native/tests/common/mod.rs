//! Shared by the tests: the recorded `gh` output of test/fixtures, a Hangar
//! that answers like the real one (`Host`), and a started tab that checks every
//! call it makes against the permissions its manifest gives it.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};

use hangar_kit_sdk::api::{self, KitError};
use hangar_kit_sdk::format;
use hangar_kit_sdk::testing::{self, Harness, StartExt};
use hangar_kit_sdk::ui::abi::Call;
use hangar_kit_sdk::ui::{Element, HandlerId};
use kit_github_tab::Github;
use serde_json::{Value, json};

pub const MANIFEST: &str = include_str!("../../../hangar-kit.json");

pub fn fixture(name: &str) -> &'static str {
    match name {
        "issue-list.json" => include_str!("../../../test/fixtures/issue-list.json"),
        "issue-view.json" => include_str!("../../../test/fixtures/issue-view.json"),
        "no-repo.stderr" => include_str!("../../../test/fixtures/no-repo.stderr"),
        "not-a-git-repo.stderr" => include_str!("../../../test/fixtures/not-a-git-repo.stderr"),
        "not-logged-in.stderr" => include_str!("../../../test/fixtures/not-logged-in.stderr"),
        "pr-list.json" => include_str!("../../../test/fixtures/pr-list.json"),
        "pr-threads.json" => include_str!("../../../test/fixtures/pr-threads.json"),
        "pr-view-failing.json" => include_str!("../../../test/fixtures/pr-view-failing.json"),
        "repo-info.json" => include_str!("../../../test/fixtures/repo-info.json"),
        other => panic!("no fixture {other}"),
    }
}

pub fn json_fixture(name: &str) -> Value {
    serde_json::from_str(fixture(name)).expect("a fixture is JSON")
}

/// The manifest's permissions for one of the tabs.
pub fn permissions(tab: &str) -> Vec<String> {
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    manifest["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == tab)
        .unwrap_or_else(|| panic!("no tab {tab}"))["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|permission| permission.as_str().unwrap().to_owned())
        .collect()
}

/// Text with the no-break spaces of Hangar's typography as plain spaces.
pub fn plain(text: &str) -> String {
    text.replace(['\u{a0}', '\u{202f}'], " ")
}

/// A moment as milliseconds since the epoch.
pub fn at(iso: &str) -> i64 {
    format::parse_iso(iso).expect("an ISO date")
}

/// When the tests say it is: the clock every test starts from.
pub fn now() -> i64 {
    at("2026-10-31T17:02:00Z")
}

// ------------------------------------------------------------------- Host

/// What a command answers: the fields it sets over `{ exitCode: 0, stdout: "",
/// stderr: "", timedOut: false, truncated: false }`, or an error of Hangar's.
pub type Answer = Result<Value, KitError>;
type Command = Box<dyn FnMut(&BTreeMap<String, String>) -> Answer>;

/// A command's answer with every field Hangar gives, from the ones that matter.
pub fn complete(answer: Answer) -> Answer {
    let mut full = json!({
        "exitCode": 0, "stdout": "", "stderr": "", "timedOut": false, "truncated": false
    });
    for (key, value) in answer?.as_object().cloned().unwrap_or_default() {
        full[key] = value;
    }
    Ok(full)
}

pub fn ok(name: &str) -> Answer {
    Ok(json!({ "exitCode": 0, "stdout": fixture(name) }))
}

pub fn failed(name: &str, exit_code: i32) -> Answer {
    Ok(json!({ "exitCode": exit_code, "stderr": fixture(name) }))
}

pub fn refused(code: &str) -> Answer {
    Err(KitError::new(code, code))
}

/// A fake Hangar: commands by name, the panes,
/// the kit's data folder in memory, and a log of every call it answered.
pub struct Host {
    pub log: Vec<Call>,
    commands: BTreeMap<String, Command>,
    pub panes: Value,
    pub files: BTreeMap<String, String>,
    pub deny_send: bool,
    pub deny_panes: bool,
    pub deny_data: bool,
}

impl Host {
    /// The commands of a repository with the recorded answers.
    pub fn recorded() -> Host {
        let mut host = Host::bare();
        host.command("repo-info", |_| ok("repo-info.json"));
        host.command("pr-list", |_| ok("pr-list.json"));
        host.command("pr-view", |_| ok("pr-view-failing.json"));
        host.command("pr-threads", |_| ok("pr-threads.json"));
        host.command("issue-list", |_| ok("issue-list.json"));
        host.command("issue-view", |_| ok("issue-view.json"));
        host
    }

    /// No command: each one is refused.
    pub fn bare() -> Host {
        Host {
            log: Vec::new(),
            commands: BTreeMap::new(),
            panes: json!([
                { "name": "Ava", "agent": "claude", "role": "worker", "state": "idle", "canSend": true },
                { "name": "Term", "agent": "shell", "role": null, "state": null, "canSend": false },
                { "name": "Max", "agent": "claude", "role": "orchestrator", "state": "working", "canSend": true },
            ]),
            files: BTreeMap::new(),
            deny_send: false,
            deny_panes: false,
            deny_data: false,
        }
    }

    pub fn command(
        &mut self,
        name: &str,
        answer: impl FnMut(&BTreeMap<String, String>) -> Answer + 'static,
    ) {
        self.commands.insert(name.to_owned(), Box::new(answer));
    }

    pub fn with_data(mut self, path: &str, text: &str) -> Host {
        self.files.insert(path.to_owned(), text.to_owned());
        self
    }

    /// Every call to `method` answered so far.
    pub fn calls_to(&self, method: &str) -> Vec<&Call> {
        self.log
            .iter()
            .filter(|call| call.method == method)
            .collect()
    }

    /// Every run of the command `name`.
    pub fn runs(&self, name: &str) -> Vec<&Call> {
        self.log
            .iter()
            .filter(|call| call.method == "command.run" && call.params["name"] == name)
            .collect()
    }

    /// The params of the badges set, in order.
    pub fn badges(&self) -> Vec<Value> {
        self.calls_to("tab.setBadge")
            .iter()
            .map(|call| call.params.clone())
            .collect()
    }

    fn answer(&mut self, call: &Call) -> Answer {
        let denied = || Err(KitError::new(KitError::DENIED, "denied"));
        match call.method.as_str() {
            "tab.setBadge" | "tab.setTitle" => Ok(json!({})),
            "panes.list" if self.deny_panes => denied(),
            "panes.list" => Ok(self.panes.clone()),
            "panes.send" if self.deny_send => denied(),
            "panes.send" => Ok(json!({ "queued": true, "pane": call.params["pane"] })),
            "data.read" | "data.write" if self.deny_data => denied(),
            "data.read" => {
                let path = call.params["path"].as_str().unwrap();
                match self.files.get(path) {
                    Some(text) => Ok(json!({ "text": text })),
                    None => Err(KitError::new(KitError::NOT_FOUND, "no such file")),
                }
            }
            "data.write" => {
                self.files.insert(
                    call.params["path"].as_str().unwrap().to_owned(),
                    call.params["text"].as_str().unwrap().to_owned(),
                );
                Ok(json!({}))
            }
            "command.run" => {
                let name = call.params["name"].as_str().unwrap();
                let args: BTreeMap<String, String> = call
                    .params
                    .get("args")
                    .and_then(|args| serde_json::from_value(args.clone()).ok())
                    .unwrap_or_default();
                let Some(command) = self.commands.get_mut(name) else {
                    return Err(KitError::new(
                        KitError::DENIED,
                        format!("no such command {name}"),
                    ));
                };
                complete(command(&args))
            }
            "ui.toast" => Ok(json!({ "action": false })),
            "ui.copy" => Ok(json!({})),
            other => Err(KitError::new(KitError::UNKNOWN, format!("unknown {other}"))),
        }
    }
}

// -------------------------------------------------------------------- Tab

/// A tab started the way Hangar starts it: with what its manifest grants, the
/// clock of `now()`, in English. Every call it makes is held to those
/// permissions, as Hangar's gate does.
pub struct Tab {
    harness: Harness<Github>,
    granted: Vec<String>,
}

impl Deref for Tab {
    type Target = Harness<Github>;
    fn deref(&self) -> &Self::Target {
        &self.harness
    }
}

impl DerefMut for Tab {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.harness
    }
}

impl Tab {
    /// `pulls` or `issues`, in English, its saved filters read and the list
    /// asked for once `settle` has answered.
    pub fn start(id: &str) -> Tab {
        Tab::start_in(id, "en")
    }

    pub fn start_in(id: &str, locale: &str) -> Tab {
        Tab::start_with(id, locale, true)
    }

    /// Started behind another tab.
    pub fn start_hidden(id: &str) -> Tab {
        Tab::start_with(id, "en", false)
    }

    fn start_with(id: &str, locale: &str, visible: bool) -> Tab {
        testing::set_now(Some(now()));
        let granted = permissions(id);
        let mut start = testing::start().grant(granted.clone()).locale(locale);
        start.tab.id = id.to_owned();
        start.visible = visible;
        Tab {
            harness: Harness::new(start),
            granted,
        }
    }

    /// Answers each call as `host` does, and the calls those make, until none
    /// waits. Each call is checked against the manifest's permissions first.
    pub fn settle(&mut self, host: &mut Host) {
        for _ in 0..200 {
            let Some(call) = self.harness.calls().first().cloned() else {
                return;
            };
            self.permitted(&call);
            let answer = host.answer(&call);
            host.log.push(call.clone());
            self.harness.reply(call.id, answer);
        }
        panic!("the tab keeps calling: {:?}", self.harness.methods());
    }

    /// Starts the tab and answers everything it asks for.
    pub fn started(id: &str, host: &mut Host) -> Tab {
        let mut tab = Tab::start(id);
        tab.settle(host);
        tab
    }

    /// Answers the oldest call to `method` by hand.
    pub fn reply_to(&mut self, method: &str, answer: Answer) -> Call {
        if let Some(call) = self
            .harness
            .calls()
            .iter()
            .find(|call| call.method == method)
        {
            self.permitted(call);
        }
        self.harness.reply_to(method, answer)
    }

    pub fn permitted(&self, call: &Call) {
        assert!(
            api::may_call(&call.method, &call.params, &self.granted),
            "{} {} is not granted by the manifest: {:?}",
            call.method,
            call.params,
            self.granted
        );
    }

    pub fn set_now(&self, iso: &str) {
        testing::set_now(Some(at(iso)));
    }

    /// The tab goes behind another, or comes back.
    pub fn visible(&mut self, visible: bool) {
        self.harness
            .event("tab.visibility", json!({ "visible": visible }));
    }

    // ------------------------------------------------------------ the page

    /// The refresh timer's period, when it is declared.
    pub fn refresh_every(&self) -> Option<u32> {
        self.subscriptions()
            .timers
            .iter()
            .find(|timer| timer.id == "refresh")
            .map(|timer| timer.every_ms)
    }

    /// The numbers of the rows of the list, in the order drawn.
    pub fn row_numbers(&self) -> Vec<i64> {
        let mut numbers = Vec::new();
        self.view().root.for_each(&mut |element| {
            if let Element::List(list) = element {
                for item in &list.items {
                    let first = item.sub.as_deref().and_then(|sub| sub.split(' ').next());
                    if let Some(number) = first.and_then(|word| word.strip_prefix('#')) {
                        numbers.extend(number.parse::<i64>());
                    }
                }
            }
        });
        numbers
    }

    /// The titles and second lines of every row of every list.
    pub fn rows(&self) -> Vec<(String, Option<String>)> {
        let mut rows = Vec::new();
        self.view().root.for_each(&mut |element| {
            if let Element::List(list) = element {
                rows.extend(
                    list.items
                        .iter()
                        .map(|item| (item.title.clone(), item.sub.clone())),
                );
            }
        });
        rows
    }

    /// Every text of the page, joined: for `contains`. Bodies of rows and
    /// values of key-values included; the no-break spaces Hangar's formats
    /// and typography write are plain spaces here.
    pub fn text(&self) -> String {
        plain(&self.texts().join("\n"))
    }

    /// The value of a key of a page's key-value list.
    pub fn fact(&self, key: &str) -> Option<String> {
        let mut found = None;
        self.view().root.for_each(&mut |element| {
            if let Element::KeyValue(pairs) = element {
                for pair in &pairs.pairs {
                    if pair.key == key
                        && let Element::Text(text) = &pair.value
                    {
                        found = Some(text.text.clone());
                    }
                }
            }
        });
        found
    }

    /// The filter chips: label, count, whether pressed.
    pub fn filters(&self) -> Vec<(String, Option<String>, bool)> {
        let mut chips = Vec::new();
        self.view().root.for_each(&mut |element| {
            if let Element::Filter(filter) = element {
                chips.push((filter.label.clone(), filter.count.clone(), filter.pressed));
            }
        });
        chips
    }

    /// What is typed in the search field.
    pub fn query(&self) -> Option<String> {
        let mut query = None;
        self.view().root.for_each(&mut |element| {
            if let Element::Search(search) = element {
                query = Some(search.value.clone());
            }
        });
        query
    }

    /// What the search field asks for.
    pub fn search_placeholder(&self) -> Option<String> {
        let mut found = None;
        self.view().root.for_each(&mut |element| {
            if let Element::Search(search) = element {
                found = Some(search.placeholder.clone());
            }
        });
        found
    }

    /// The hand-off menus: their id, label, kind, and the lines they list as
    /// (id, label, hint, disabled).
    pub fn menus(&self) -> Vec<Menu> {
        let mut menus = Vec::new();
        self.view().root.for_each(&mut |element| {
            if let Element::Menu(menu) = element {
                menus.push(Menu {
                    id: menu.id.clone(),
                    label: menu.label.clone().unwrap_or_default(),
                    primary: menu.kind == hangar_kit_sdk::ui::ButtonKind::Primary,
                    on_open: menu.on_open,
                    lines: menu
                        .items
                        .iter()
                        .filter_map(|entry| match entry {
                            hangar_kit_sdk::ui::MenuEntry::Item(item) => Some((
                                item.id.clone(),
                                item.label.clone(),
                                item.hint.clone(),
                                item.disabled,
                            )),
                            _ => None,
                        })
                        .collect(),
                });
            }
        });
        menus
    }

    /// Opens the hand-off menu of one item, and answers what it asks.
    pub fn open_hand(&mut self, number: i64, host: &mut Host) {
        let id = format!("hand-{number}");
        let handler = self
            .menus()
            .into_iter()
            .find(|menu| menu.id == id)
            .and_then(|menu| menu.on_open)
            .unwrap_or_else(|| panic!("no menu {id} takes its opening"));
        self.harness.ui(handler, None);
        self.settle(host);
    }

    /// Whether any button, filter, link or row of the page is labelled `label`.
    pub fn has_button(&self, label: &str) -> bool {
        let mut found = false;
        self.view().root.for_each(&mut |element| {
            if let Element::Button(button) = element {
                found |= button.label.as_deref() == Some(label)
                    || button.tooltip.as_deref() == Some(label);
            }
        });
        found
    }

    /// The buttons and menus drawn as the primary action.
    pub fn primaries(&self) -> Vec<String> {
        let mut found = Vec::new();
        self.view().root.for_each(&mut |element| match element {
            Element::Button(button) if button.kind == hangar_kit_sdk::ui::ButtonKind::Primary => {
                found.push(button.label.clone().unwrap_or_default())
            }
            Element::Menu(menu) if menu.kind == hangar_kit_sdk::ui::ButtonKind::Primary => {
                found.push(menu.label.clone().unwrap_or_default())
            }
            _ => {}
        });
        found
    }

    /// The `Empty` blocks of the page: kind, title, text.
    pub fn empties(&self) -> Vec<hangar_kit_sdk::ui::Empty<HandlerId>> {
        let mut found = Vec::new();
        self.view().root.for_each(&mut |element| {
            if let Element::Empty(empty) = element {
                found.push(empty.clone());
            }
        });
        found
    }
}

#[derive(Debug, Clone)]
pub struct Menu {
    pub id: String,
    pub label: String,
    pub primary: bool,
    pub on_open: Option<HandlerId>,
    pub lines: Vec<(String, String, Option<String>, bool)>,
}
