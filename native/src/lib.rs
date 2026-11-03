//! The GitHub kit's two tabs, **Pull requests** and **Issues**, in Rust: one
//! crate, one module (`tabs/github.wasm`), `init` reading which tab it is
//! from `start.tab.id`. It only reads GitHub, through six declared `gh`
//! commands, and hands an item to an agent of the workspace with `panes.send`.
//!
//! - `logic`: gh's JSON into rows and details, the filters, the badge, the
//!   brief an agent receives (what GitHub's users wrote, quoted and marked as
//!   not instructions). Pure, and tested without a tab.
//! - `strings`: English and French.
//! - `view`: the page, as a function of the state.
//! - this file: the state, the messages, the calls.
//!
//! The filters are saved in the kit's data folder, in the format the earlier
//! HTML pages of this kit wrote, so that saved filters carry over.

use std::collections::BTreeMap;
use std::time::Duration;

use hangar_kit_sdk::api::{CommandAnswer, DataText, Done, Pane, Queued, Theme};
use hangar_kit_sdk::prelude::*;
use serde_json::Value;

pub mod logic;
pub mod strings;
mod view;

use logic::{
    Badge, BadgeKind, Detail, FailKind, Failure, Filters, IssueRow, PullRow, Tab, Threads,
};

/// The list is read again this often while the tab is in front.
pub const REFRESH: Duration = Duration::from_secs(60);
/// Filters are saved this long after the last change (`data.write` is
/// limited to 10 a minute).
pub const SAVE_DELAY: Duration = Duration::from_millis(700);
/// A tab brought back in view, or a list come back to, does not read again
/// within this many milliseconds of the last read.
pub const MIN_GAP_MS: i64 = 5_000;

/// What the page is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Loading,
    List,
    Detail,
    Error,
}

/// The repository of the workspace's folder, as `repo-info` says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub owner: String,
    pub name: String,
    pub name_with_owner: String,
}

enum Rows {
    Pulls(Vec<PullRow>),
    Issues(Vec<IssueRow>),
}

impl Rows {
    fn is_empty(&self) -> bool {
        match self {
            Rows::Pulls(rows) => rows.is_empty(),
            Rows::Issues(rows) => rows.is_empty(),
        }
    }
}

/// One item opened in full, or on its way.
struct DetailState {
    number: i64,
    loading: bool,
    error: Option<Failure>,
    data: Option<Detail>,
}

/// Why an item is being read.
enum Purpose {
    /// To show it: the detail it was asked for under (`detail_generation`).
    Open(u64),
    /// To hand it to this pane.
    Brief(String),
}

/// An item being read: its view, and its review threads for a pull request.
struct Fetch {
    number: i64,
    purpose: Purpose,
    view: Option<Result<Value, Failure>>,
    /// `Some(None)` when there are none to read or they could not be.
    threads: Option<Option<Threads>>,
}

/// The agents a "Hand to an agent" menu offers once it is opened.
enum Agents {
    Reading,
    Ready(Vec<Pane>),
    NoAgent,
    Denied,
}

struct HandMenu {
    number: i64,
    agents: Agents,
}

/// A filter that is on or off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    Failing,
    Review,
    HideDrafts,
    Unassigned,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// The saved filters, or why there are none.
    FiltersRead(Result<DataText, KitError>),
    Repo(u64, Result<CommandAnswer, KitError>),
    List(u64, Result<CommandAnswer, KitError>),
    Refresh,
    Visible(bool),
    Theme(Theme),
    Toggle(Flag),
    Query(String),
    ClearFilters,
    SaveFilters,
    Open(i64),
    Back,
    DetailView(u64, Result<CommandAnswer, KitError>),
    DetailThreads(u64, Result<CommandAnswer, KitError>),
    /// The hand-off menu of this item opened.
    AgentsOpened(i64),
    Agents(u64, Result<Vec<Pane>, KitError>),
    /// This item, to this pane.
    Hand(i64, String),
    Sent(String, Result<Queued, KitError>),
    Copy(String),
    CopyCommand(String),
    /// A copy answered; the key of the toast to say.
    Copied(&'static str, Result<Done, KitError>),
}

pub struct Github {
    tab: Tab,
    t: Strings,
    now: i64,
    utc_offset: i32,
    screen: Screen,
    rows: Rows,
    repo: Option<Repo>,
    filters: Filters,
    error: Option<Failure>,
    /// A refresh failed; the last list stays.
    stale: bool,
    refreshing: bool,
    refreshed_at: Option<i64>,
    detail: Option<DetailState>,
    /// The item being handed over: its buttons wait.
    busy: Option<i64>,
    /// A later refresh makes the answer to an earlier one stale.
    generation: u64,
    /// So does a later detail.
    detail_generation: u64,
    fetches: BTreeMap<u64, Fetch>,
    next_fetch: u64,
    hand: Option<HandMenu>,
    hand_generation: u64,
    /// The tab is in front: the list is read again every minute.
    visible: bool,
    last_badge: Option<String>,
}

impl Github {
    /// Starts reading the list again: the repository first, once.
    fn refresh(&mut self, cx: &mut Context<Message>) {
        self.generation += 1;
        let token = self.generation;
        self.refreshing = true;
        if self.screen == Screen::Error {
            self.screen = Screen::Loading;
        }
        if self.repo.is_none() {
            cx.call(run("repo-info", &[]), move |answer| {
                Message::Repo(token, answer)
            });
        } else {
            self.ask_list(token, cx);
        }
    }

    fn ask_list(&self, token: u64, cx: &mut Context<Message>) {
        let command = match self.tab {
            Tab::Pulls => "pr-list",
            Tab::Issues => "issue-list",
        };
        cx.call(run(command, &[]), move |answer| {
            Message::List(token, answer)
        });
    }

    /// What a read of the list came to. Shown over the previous list, which
    /// stays if this failed and the list is what is showing.
    fn finish_refresh(&mut self, outcome: Result<Rows, Failure>, cx: &mut Context<Message>) {
        self.refreshing = false;
        match outcome {
            Ok(rows) => {
                self.rows = rows;
                self.error = None;
                self.stale = false;
                self.refreshed_at = Some(cx.now());
                if matches!(self.screen, Screen::Loading | Screen::Error) {
                    self.screen = Screen::List;
                }
                let badge = match &self.rows {
                    Rows::Pulls(rows) => logic::badge_for(rows, &self.t),
                    Rows::Issues(_) => None,
                };
                self.set_badge(badge, cx);
            }
            Err(failure) => {
                // A blip does not erase a list that is showing. (The page also
                // kept a list behind its loading screen when a retry failed;
                // here a failure with nothing to show is drawn.)
                let keeps = match self.screen {
                    Screen::Detail => true,
                    Screen::List => !self.rows.is_empty() && !logic::is_fatal(failure.kind),
                    Screen::Loading | Screen::Error => false,
                };
                if keeps {
                    self.stale = true;
                } else {
                    self.screen = Screen::Error;
                    self.error = Some(failure);
                    self.stale = false;
                    self.set_badge(None, cx);
                }
            }
        }
    }

    /// The tab's badge is its own business and never worth failing for.
    fn set_badge(&mut self, badge: Option<Badge>, cx: &mut Context<Message>) {
        let key = badge
            .as_ref()
            .map(|badge| format!("{:?}|{}", badge.kind, badge.text))
            .unwrap_or_else(|| "none".to_owned());
        if self.last_badge.as_deref() == Some(&key) {
            return;
        }
        self.last_badge = Some(key);
        cx.call_quietly(api::TabSetBadge {
            state: badge.as_ref().map(|badge| match badge.kind {
                BadgeKind::Failure => api::BadgeState::Failure,
                BadgeKind::Neutral => api::BadgeState::Neutral,
            }),
            text: badge.map(|badge| badge.text),
        });
    }

    fn save_soon(&self, cx: &mut Context<Message>) {
        cx.after("save", SAVE_DELAY, Message::SaveFilters);
    }

    /// Shows one item in full, once it is read.
    fn open(&mut self, number: i64, cx: &mut Context<Message>) {
        self.detail_generation += 1;
        self.screen = Screen::Detail;
        self.detail = Some(DetailState {
            number,
            loading: true,
            error: None,
            data: None,
        });
        let token = self.detail_generation;
        self.fetch(number, Purpose::Open(token), cx);
    }

    /// Reads one item in full: its view, and for a pull request its review
    /// threads, both at once.
    fn fetch(&mut self, number: i64, purpose: Purpose, cx: &mut Context<Message>) {
        self.next_fetch += 1;
        let id = self.next_fetch;
        let given = number.to_string();
        let (command, threads) = match (self.tab, &self.repo) {
            (Tab::Issues, _) => ("issue-view", Some(None)),
            (Tab::Pulls, None) => ("pr-view", Some(None)),
            (Tab::Pulls, Some(repo)) => {
                cx.call(
                    run(
                        "pr-threads",
                        &[
                            ("id", given.as_str()),
                            ("owner", repo.owner.as_str()),
                            ("name", repo.name.as_str()),
                        ],
                    ),
                    move |answer| Message::DetailThreads(id, answer),
                );
                ("pr-view", None)
            }
        };
        cx.call(run(command, &[("id", &given)]), move |answer| {
            Message::DetailView(id, answer)
        });
        self.fetches.insert(
            id,
            Fetch {
                number,
                purpose,
                view: None,
                threads,
            },
        );
    }

    /// When both of an item's answers are in, what it comes to.
    fn fetched(&mut self, id: u64, cx: &mut Context<Message>) {
        let Some(fetch) = self.fetches.get(&id) else {
            return;
        };
        if fetch.view.is_none() || fetch.threads.is_none() {
            return;
        }
        let Some(Fetch {
            number,
            purpose,
            view,
            threads,
        }) = self.fetches.remove(&id)
        else {
            return;
        };
        let result = match view {
            Some(Ok(value)) => Ok(match self.tab {
                Tab::Pulls => Detail::Pull(logic::pull_detail(&value, threads.flatten())),
                Tab::Issues => Detail::Issue(logic::issue_detail(&value)),
            }),
            Some(Err(failure)) => Err(failure),
            None => return,
        };
        match purpose {
            Purpose::Open(token) => {
                if token != self.detail_generation {
                    return;
                }
                self.detail = Some(match result {
                    Ok(data) => DetailState {
                        number,
                        loading: false,
                        error: None,
                        data: Some(data),
                    },
                    Err(failure) => DetailState {
                        number,
                        loading: false,
                        error: Some(failure),
                        data: None,
                    },
                });
            }
            Purpose::Brief(pane) => match result {
                Ok(data) => self.send_brief(&data, pane, cx),
                Err(_) => {
                    self.busy = None;
                    toast(cx, Notice::Error, self.t.t("briefFailed"));
                }
            },
        }
    }

    fn send_brief(&mut self, data: &Detail, pane: String, cx: &mut Context<Message>) {
        let repo = self
            .repo
            .as_ref()
            .map(|repo| repo.name_with_owner.as_str())
            .unwrap_or("");
        let text = logic::build_brief(data, repo);
        let named = pane.clone();
        cx.call(api::PanesSend { pane, text }, move |answer| {
            Message::Sent(named.clone(), answer)
        });
    }
}

impl Kit for Github {
    type Message = Message;

    fn init(start: &Start, cx: &mut Context<Message>) -> Self {
        let tab = Tab::from_id(&start.tab.id);
        // The saved filters first, then the list: what is asked for is what
        // the person had set.
        cx.call(
            api::DataRead {
                path: logic::filters_file(tab).to_owned(),
            },
            Message::FiltersRead,
        );
        Github {
            tab,
            t: strings::strings(&start.theme.locale),
            now: cx.now(),
            utc_offset: start.utc_offset,
            screen: Screen::Loading,
            rows: match tab {
                Tab::Pulls => Rows::Pulls(Vec::new()),
                Tab::Issues => Rows::Issues(Vec::new()),
            },
            repo: None,
            filters: Filters::default(),
            error: None,
            stale: false,
            refreshing: false,
            refreshed_at: None,
            detail: None,
            busy: None,
            generation: 0,
            detail_generation: 0,
            fetches: BTreeMap::new(),
            next_fetch: 0,
            hand: None,
            hand_generation: 0,
            visible: start.visible,
            last_badge: None,
        }
    }

    fn update(&mut self, message: Message, cx: &mut Context<Message>) {
        self.now = cx.now();
        match message {
            Message::FiltersRead(answer) => {
                // None saved yet, or not granted: the defaults.
                if let Ok(read) = answer
                    && let Ok(saved) = serde_json::from_str::<Value>(&read.text)
                {
                    self.filters = logic::sanitize_filters(self.tab, &saved);
                }
                self.refresh(cx);
            }
            Message::Repo(token, answer) => match command_json(answer) {
                Ok(info) => {
                    self.repo = Some(repo_of(&info));
                    if token == self.generation {
                        self.ask_list(token, cx);
                    }
                }
                Err(failure) => {
                    if token == self.generation {
                        self.finish_refresh(Err(failure), cx);
                    }
                }
            },
            Message::List(token, answer) => {
                if token != self.generation {
                    return;
                }
                let outcome = command_json(answer).and_then(|list| match (&list, self.tab) {
                    (Value::Array(_), Tab::Pulls) => Ok(Rows::Pulls(logic::pull_rows(&list))),
                    (Value::Array(_), Tab::Issues) => Ok(Rows::Issues(logic::issue_rows(&list))),
                    _ => Err(Failure {
                        kind: FailKind::Error,
                        detail: list.to_string().chars().take(300).collect(),
                    }),
                });
                self.finish_refresh(outcome, cx);
            }
            Message::Refresh => self.refresh(cx),
            Message::Visible(visible) => {
                let was = self.visible;
                self.visible = visible;
                let due = self
                    .refreshed_at
                    .is_none_or(|at| cx.now() - at > MIN_GAP_MS);
                if visible && !was && self.screen != Screen::Detail && due {
                    self.refresh(cx);
                }
            }
            Message::Theme(theme) => {
                let before = strings::pick_lang(self.t.locale());
                self.t.set_locale(strings::pick_lang(&theme.locale));
                if before != strings::pick_lang(&theme.locale) {
                    self.last_badge = None;
                    if let Rows::Pulls(rows) = &self.rows
                        && !rows.is_empty()
                    {
                        let badge = logic::badge_for(rows, &self.t);
                        self.set_badge(badge, cx);
                    }
                }
            }
            Message::Toggle(flag) => {
                let filters = &mut self.filters;
                match flag {
                    Flag::Failing => filters.failing = !filters.failing,
                    Flag::Review => filters.review = !filters.review,
                    Flag::HideDrafts => filters.hide_drafts = !filters.hide_drafts,
                    Flag::Unassigned => filters.unassigned = !filters.unassigned,
                }
                self.save_soon(cx);
            }
            Message::Query(query) => {
                self.filters.query = query.chars().take(200).collect();
                self.save_soon(cx);
            }
            Message::ClearFilters => {
                self.filters = Filters::default();
                self.save_soon(cx);
            }
            Message::SaveFilters => cx.call_quietly(api::DataWrite {
                path: logic::filters_file(self.tab).to_owned(),
                text: logic::filters_json(self.tab, &self.filters),
            }),
            Message::Open(number) => self.open(number, cx),
            Message::Back => {
                self.detail_generation += 1;
                self.screen = Screen::List;
                self.detail = None;
                if self
                    .refreshed_at
                    .is_none_or(|at| cx.now() - at > MIN_GAP_MS)
                {
                    self.refresh(cx);
                }
            }
            Message::DetailView(id, answer) => {
                if let Some(fetch) = self.fetches.get_mut(&id) {
                    fetch.view = Some(command_json(answer));
                }
                self.fetched(id, cx);
            }
            Message::DetailThreads(id, answer) => {
                if let Some(fetch) = self.fetches.get_mut(&id) {
                    // Threads that cannot be read do not hide the item.
                    fetch.threads = Some(
                        command_json(answer)
                            .ok()
                            .and_then(|v| logic::parse_threads(&v)),
                    );
                }
                self.fetched(id, cx);
            }
            Message::AgentsOpened(number) => {
                self.hand_generation += 1;
                let token = self.hand_generation;
                self.hand = Some(HandMenu {
                    number,
                    agents: Agents::Reading,
                });
                cx.call(api::PanesList {}, move |answer| {
                    Message::Agents(token, answer)
                });
            }
            Message::Agents(token, answer) => {
                if token != self.hand_generation {
                    return;
                }
                let agents = match answer {
                    Ok(panes) => {
                        let ready: Vec<Pane> = panes.into_iter().filter(sendable).collect();
                        if ready.is_empty() {
                            Agents::NoAgent
                        } else {
                            Agents::Ready(ready)
                        }
                    }
                    Err(error) if error.is(KitError::DENIED) => Agents::Denied,
                    Err(_) => Agents::NoAgent,
                };
                match agents {
                    Agents::Denied => toast(cx, Notice::Warning, self.t.t("listDenied")),
                    Agents::NoAgent => toast(cx, Notice::Warning, self.t.t("noAgent")),
                    _ => {}
                }
                if let Some(hand) = &mut self.hand {
                    hand.agents = agents;
                }
            }
            Message::Hand(number, pane) => {
                if self.busy.is_some() {
                    return;
                }
                self.busy = Some(number);
                let open = match &self.detail {
                    Some(detail) if detail.number == number => detail.data.clone(),
                    _ => None,
                };
                match open {
                    Some(data) => self.send_brief(&data, pane, cx),
                    None => self.fetch(number, Purpose::Brief(pane), cx),
                }
            }
            Message::Sent(pane, answer) => {
                self.busy = None;
                match answer {
                    Ok(_) => toast(cx, Notice::Success, self.t.tv("sent", &[("pane", &pane)])),
                    Err(error) if error.is(KitError::DENIED) => {
                        toast(cx, Notice::Error, self.t.t("sendDenied"))
                    }
                    Err(error) => toast(
                        cx,
                        Notice::Error,
                        self.t.tv("sendFailed", &[("detail", &error.message)]),
                    ),
                }
            }
            Message::Copy(text) => cx.call(api::CopyText { text }, |answer| {
                Message::Copied("copied", answer)
            }),
            Message::CopyCommand(text) => cx.call(api::CopyText { text }, |answer| {
                Message::Copied("copiedCommand", answer)
            }),
            Message::Copied(key, Ok(_)) => toast(cx, Notice::Success, self.t.t(key)),
            Message::Copied(_, Err(_)) => {}
        }
    }

    fn view(&self) -> Element<Message> {
        self.page()
    }

    fn subscriptions(&self) -> Subscriptions<Message> {
        let subscriptions = Subscriptions::none()
            .visibility(Message::Visible)
            .theme(Message::Theme);
        // Behind another tab, nobody reads it: it is read again on return.
        // The minute goes on in every screen, an error included, in case the
        // person fixed what was wrong.
        if self.visible {
            subscriptions.every("refresh", REFRESH, Message::Refresh)
        } else {
            subscriptions
        }
    }
}

/// A pane a brief can be sent to: a Claude Code agent that can receive now.
pub fn sendable(pane: &Pane) -> bool {
    pane.can_send && pane.agent == "claude"
}

/// A toast for what just happened, kept under the 500 characters Hangar takes.
fn toast(cx: &mut Context<Message>, kind: Notice, message: String) {
    let message = if message.chars().count() > 480 {
        let mut cut: String = message.chars().take(479).collect();
        cut.push('…');
        cut
    } else {
        message
    };
    cx.toast(kind, message);
}

/// `command.run` of a declared command.
fn run(name: &str, args: &[(&str, &str)]) -> api::CommandRun {
    api::CommandRun {
        name: name.to_owned(),
        args: args
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
    }
}

/// A command's answer: its text when it ended well, else why not.
fn command_text(answer: Result<CommandAnswer, KitError>) -> Result<String, Failure> {
    match answer {
        Err(error) => Err(Failure {
            kind: logic::classify_error(&error.code),
            detail: error.message,
        }),
        Ok(done) if done.exit_code == Some(0) => Ok(done.stdout),
        Ok(done) => {
            let said = if done.stderr.is_empty() {
                &done.stdout
            } else {
                &done.stderr
            };
            Err(Failure {
                kind: logic::classify_exit(&done.stderr, done.timed_out),
                detail: logic::strip_ansi(said).trim().chars().take(500).collect(),
            })
        }
    }
}

fn command_json(answer: Result<CommandAnswer, KitError>) -> Result<Value, Failure> {
    let text = command_text(answer)?;
    serde_json::from_str(&text).map_err(|_| Failure {
        kind: FailKind::Error,
        detail: text.chars().take(300).collect(),
    })
}

/// `repo-info`'s answer.
fn repo_of(info: &Value) -> Repo {
    let owner = match info.get("owner") {
        Some(Value::String(owner)) => owner.clone(),
        Some(owner) => owner
            .get("login")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        None => String::new(),
    };
    let name = info
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let given = info
        .get("nameWithOwner")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let name_with_owner = if !given.is_empty() {
        given.to_owned()
    } else if !owner.is_empty() && !name.is_empty() {
        format!("{owner}/{name}")
    } else {
        name.clone()
    };
    Repo {
        owner,
        name,
        name_with_owner,
    }
}

export_kit!(Github);
