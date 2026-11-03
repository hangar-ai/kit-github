//! The page, as a function of the state: the list of the open pull requests or
//! issues, one of them in full, or why neither could be read. Hangar draws
//! the widgets in the person's theme; this names what each thing is, never a
//! colour. Text from GitHub is only ever text.

use hangar_kit_sdk::prelude::*;
use hangar_kit_sdk::ui::MenuEntry;

use crate::logic::{
    self, CheckState, Checks, ChecksState, Detail, FailKind, Failure, IssueDetail, IssueRow,
    PullDetail, PullRow, Review, Tab,
};
use crate::{Agents, Flag, Github, Message, Rows, Screen};

type El = Element<Message>;

/// The description is cut here, as the page cut it.
const BODY_SHOWN: usize = 4000;
const FILES_SHOWN: usize = 30;

fn status_of(state: ChecksState) -> Status {
    match state {
        ChecksState::Failure => Status::Failure,
        ChecksState::Running => Status::Running,
        ChecksState::Success => Status::Success,
        ChecksState::None | ChecksState::Neutral => Status::Neutral,
    }
}

fn status_of_check(state: CheckState) -> Status {
    match state {
        CheckState::Failure => Status::Failure,
        CheckState::Running => Status::Running,
        CheckState::Success => Status::Success,
        CheckState::Neutral => Status::Neutral,
    }
}

/// A line of text, flattened and cut: what a row shows of a comment.
fn excerpt(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        format!("{}…", flat.chars().take(max).collect::<String>())
    } else {
        flat
    }
}

fn or_question(name: &str) -> String {
    if name.is_empty() {
        "?".to_owned()
    } else {
        name.to_owned()
    }
}

/// Parts of a line of state, a dot between each.
fn dotted(parts: Vec<El>) -> Vec<El> {
    let mut out = Vec::with_capacity(parts.len() * 2);
    for (index, part) in parts.into_iter().enumerate() {
        if index > 0 {
            out.push(text("·").dim().into());
        }
        out.push(part);
    }
    out
}

impl Github {
    pub(crate) fn page(&self) -> El {
        match self.screen {
            Screen::Loading => page![loading(self.t.t("loading"))].into(),
            Screen::Error => match &self.error {
                Some(failure) => page![self.problem(failure, Message::Refresh)].into(),
                None => page![loading(self.t.t("loading"))].into(),
            },
            Screen::Detail => self.detail_page(),
            Screen::List => self.list_page(),
        }
    }

    // ------------------------------------------------------------ shared

    fn checks_word(&self, checks: &Checks) -> String {
        let t = &self.t;
        match checks.state {
            ChecksState::Failure => t.tn("checkFailing", checks.failing.len() as i64, &[]),
            ChecksState::Running => t.tn("checksPending", checks.pending as i64, &[]),
            ChecksState::Success => t.t("checksPassing"),
            ChecksState::None | ChecksState::Neutral => t.t("checksNone"),
        }
    }

    fn review_word(&self, review: Review) -> Option<String> {
        let key = match review {
            Review::Approved => "reviewApproved",
            Review::Changes => "reviewChanges",
            Review::Required => "reviewRequired",
            Review::Requested => "reviewRequested",
            Review::None => return None,
        };
        Some(self.t.t(key))
    }

    fn state_word(&self, state: CheckState) -> String {
        self.t.t(match state {
            CheckState::Failure => "stateFailed",
            CheckState::Running => "stateRunning",
            CheckState::Success => "statePassed",
            CheckState::Neutral => "stateSkipped",
        })
    }

    /// How long ago a date of gh's was, in the person's words.
    fn since(&self, at: &Option<String>) -> String {
        at.as_deref()
            .and_then(format::parse_iso)
            .map(|at| format::since(at, self.now, self.utc_offset, self.t.locale()))
            .unwrap_or_default()
    }

    /// Why a read failed and what to do, with a button to try again: the
    /// command that fixes it can be copied, since text cannot be selected.
    fn problem(&self, failure: &Failure, retry: Message) -> El {
        let t = &self.t;
        let info = logic::problem(failure.kind, t, &failure.detail);
        let mut block: ui_empty::Empty = if info.lock {
            denied(info.title)
        } else {
            failed(info.title)
        };
        block = block.text(info.text);
        if let Some(command) = info.command {
            block = block.child(
                row![
                    text(command).code(),
                    icon_button(Icon::Copy, t.t("copyCommand"))
                        .small()
                        .on_press(Message::CopyCommand(command.to_owned())),
                ]
                .spacing(6.0)
                .align(Align::Center),
            );
        }
        if !info.detail.is_empty() {
            block = block.child(pre(info.detail).height(120.0));
        }
        // Retrying a refusal does nothing.
        if failure.kind != FailKind::Denied {
            block = block.action(button(t.t("retry")).small().primary().on_press(retry));
        }
        block.into()
    }

    /// "Hand to an agent": a menu that asks which agents there are when it
    /// opens, and lists them as they come. While one item is being handed
    /// over every one of them waits.
    fn hand_control(&self, number: i64, primary: bool) -> El {
        let t = &self.t;
        let kind = if primary {
            ButtonKind::Primary
        } else {
            ButtonKind::Default
        };
        if let Some(busy) = self.busy {
            let label = if busy == number {
                t.t("sending")
            } else {
                t.t("hand")
            };
            return button(label).small().kind(kind).into();
        }
        let waiting = |label: String| vec![menu_item("", label).disabled(true).into()];
        let entries: Vec<MenuEntry> = match &self.hand {
            Some(hand) if hand.number == number => match &hand.agents {
                Agents::Reading => waiting(t.t("readingAgents")),
                Agents::NoAgent | Agents::Denied => waiting(t.t("noAgentShort")),
                Agents::Ready(panes) => panes
                    .iter()
                    .map(|pane| {
                        let hint = match pane.state.as_deref() {
                            Some("working") => Some(t.t("agentWorking")),
                            Some("waiting") => Some(t.t("agentWaiting")),
                            Some("yours") => Some(t.t("agentYours")),
                            _ => None,
                        };
                        let item = menu_item(pane.name.clone(), pane.name.clone());
                        match hint {
                            Some(hint) => item.hint(hint),
                            None => item,
                        }
                        .into()
                    })
                    .collect(),
            },
            _ => waiting(t.t("readingAgents")),
        };
        menu(entries)
            .id(format!("hand-{number}"))
            .label(t.t("hand"))
            .kind(kind)
            .on_select(move |pane| Message::Hand(number, pane))
            .on_open(Message::AgentsOpened(number))
            .into()
    }

    // -------------------------------------------------------------- list

    fn list_page(&self) -> El {
        let t = &self.t;
        let pulls = self.tab == Tab::Pulls;
        let total = match &self.rows {
            Rows::Pulls(rows) => rows.len(),
            Rows::Issues(rows) => rows.len(),
        };
        let failing = match &self.rows {
            Rows::Pulls(rows) => rows
                .iter()
                .filter(|row| row.checks.state == ChecksState::Failure)
                .count(),
            Rows::Issues(_) => 0,
        };
        let title = match &self.repo {
            Some(repo) => repo.name_with_owner.clone(),
            None => t.t(if pulls { "tabPulls" } else { "tabIssues" }),
        };
        let mut head = header(title);
        let mut state: Vec<El> = Vec::new();
        if failing > 0 {
            state.push(
                state_label(Status::Failure, t.tn("checkFailing", failing as i64, &[])).into(),
            );
        }
        state.push(text(t.tn("open", total as i64, &[])).small().dim().into());
        for part in dotted(state) {
            head = head.state(part);
        }
        let mut refresh = button(if self.refreshing {
            t.t("refreshing")
        } else {
            t.t("refresh")
        })
        .small()
        .on_press_maybe((!self.refreshing).then_some(Message::Refresh));
        if let Some(at) = self.refreshed_at {
            let when = format::since(at, self.now, self.utc_offset, t.locale());
            refresh = refresh.tooltip(t.tv("updated", &[("when", &when)]));
        }
        head = head.action(refresh);

        let mut children: Vec<El> = vec![head.into()];
        if self.stale {
            children.push(banner(Notice::Warning, t.t("stale")).into());
        }
        children.push(self.filter_bar());

        let items: Vec<ui_list::ListItem> = match &self.rows {
            Rows::Pulls(rows) => logic::filter_pulls(rows, &self.filters)
                .into_iter()
                .map(|row| self.pull_item(row))
                .collect(),
            Rows::Issues(rows) => logic::filter_issues(rows, &self.filters)
                .into_iter()
                .map(|row| self.issue_item(row))
                .collect(),
        };
        if total == 0 {
            children.push(
                empty(t.t(if pulls { "emptyPulls" } else { "emptyIssues" }))
                    .text(t.t(if pulls {
                        "emptyPullsText"
                    } else {
                        "emptyIssuesText"
                    }))
                    .into(),
            );
        } else if items.is_empty() {
            children.push(
                empty(t.t("emptyFiltered"))
                    .icon(Icon::Filter)
                    .text(t.tn("emptyFilteredText", total as i64, &[]))
                    .action(
                        button(t.t("clearFilters"))
                            .small()
                            .on_press(Message::ClearFilters),
                    )
                    .into(),
            );
        } else {
            children.push(list(items).group().into());
        }
        page(children).into()
    }

    fn filter_bar(&self) -> El {
        let t = &self.t;
        let pulls = self.tab == Tab::Pulls;
        let field = search(
            t.t(if pulls { "searchPulls" } else { "searchIssues" }),
            self.filters.query.clone(),
        )
        .id("filter-query")
        .on_input(Message::Query)
        .on_clear(Message::Query(String::new()));
        let mut chips: Vec<El> = Vec::new();
        match &self.rows {
            Rows::Pulls(rows) => {
                let failing = rows
                    .iter()
                    .filter(|row| row.checks.state == ChecksState::Failure)
                    .count();
                let review = rows.iter().filter(|row| row.needs_review).count();
                chips.push(
                    filter(t.t("fFailing"), self.filters.failing)
                        .count(failing.to_string())
                        .count_tone(if failing > 0 {
                            Tone::Danger
                        } else {
                            Tone::Default
                        })
                        .on_press(Message::Toggle(Flag::Failing))
                        .into(),
                );
                chips.push(
                    filter(t.t("fReview"), self.filters.review)
                        .count(review.to_string())
                        .on_press(Message::Toggle(Flag::Review))
                        .into(),
                );
                chips.push(
                    filter(t.t("fDrafts"), self.filters.hide_drafts)
                        .on_press(Message::Toggle(Flag::HideDrafts))
                        .into(),
                );
            }
            Rows::Issues(rows) => {
                let unassigned = rows.iter().filter(|row| row.assignees.is_empty()).count();
                chips.push(
                    filter(t.t("fUnassigned"), self.filters.unassigned)
                        .count(unassigned.to_string())
                        .on_press(Message::Toggle(Flag::Unassigned))
                        .into(),
                );
            }
        }
        col![field, row(chips).wrap().spacing(6.0)]
            .spacing(8.0)
            .into()
    }

    fn pull_item(&self, row: &PullRow) -> ui_list::ListItem {
        let t = &self.t;
        let meta = [
            format!("#{}", row.number),
            row.author.clone(),
            row.branch.clone(),
            self.since(&row.updated_at),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
        let mut second: Vec<String> = Vec::new();
        if row.draft {
            second.push(t.t("draft"));
        }
        second.push(self.checks_word(&row.checks));
        second.extend(self.review_word(row.review));
        let state = if row.checks.state == ChecksState::None {
            Status::Neutral
        } else {
            status_of(row.checks.state)
        };
        item(row.title.clone())
            .lead(glyph(state, self.checks_word(&row.checks)))
            .sub(meta)
            .tooltip(row.title.clone())
            .body(text(second.join(" · ")).small().dim())
            .body(row![self.hand_control(row.number, false)])
            .on_press(Message::Open(row.number))
    }

    fn issue_item(&self, row: &IssueRow) -> ui_list::ListItem {
        let t = &self.t;
        let meta = [
            format!("#{}", row.number),
            row.author.clone(),
            self.since(&row.updated_at),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
        let mut second: Vec<String> = Vec::new();
        if !row.assignees.is_empty() {
            second.push(t.tv("assignedTo", &[("who", &row.assignees.join(", "))]));
        }
        if !row.labels.is_empty() {
            second.push(row.labels.join(", "));
        }
        let mut entry = item(row.title.clone())
            .lead(glyph(Status::Neutral, format!("#{}", row.number)))
            .sub(meta)
            .tooltip(row.title.clone());
        if !second.is_empty() {
            entry = entry.body(text(second.join(" · ")).small().dim());
        }
        entry
            .body(row![self.hand_control(row.number, false)])
            .on_press(Message::Open(row.number))
    }

    // ------------------------------------------------------------ detail

    fn detail_page(&self) -> El {
        let t = &self.t;
        let Some(state) = &self.detail else {
            return page![loading(t.t("loading"))].into();
        };
        let back = icon_button(Icon::ArrowLeft, t.t("back")).on_press(Message::Back);
        if state.loading {
            return page![
                header(format!("#{}", state.number)).lead(back),
                loading(t.t("loading")),
            ]
            .into();
        }
        if let Some(failure) = &state.error {
            return page![
                header(format!("#{}", state.number)).lead(back),
                self.problem(failure, Message::Open(state.number)),
            ]
            .into();
        }
        match &state.data {
            Some(Detail::Pull(data)) => self.pull_page(data, back),
            Some(Detail::Issue(data)) => self.issue_page(data, back),
            None => page![loading(t.t("loading"))].into(),
        }
    }

    /// The facts, the primary action, and the link.
    fn about(&self, number: i64, url: &str, facts: Vec<(String, String)>) -> El {
        let t = &self.t;
        let pairs: Vec<(String, El)> = facts
            .into_iter()
            .filter(|(_, value)| !value.is_empty())
            .map(|(name, value)| (name, text(value).into()))
            .collect();
        col![
            key_value(pairs),
            row![
                self.hand_control(number, true),
                button(t.t("copyLink"))
                    .small()
                    .on_press(Message::Copy(url.to_owned())),
            ]
            .spacing(8.0)
            .align(Align::Center),
            text(url.to_owned()).small().dim(),
        ]
        .spacing(8.0)
        .into()
    }

    fn description(&self, body: &str) -> El {
        let t = &self.t;
        let body = body.trim();
        let content: El = if body.is_empty() {
            text(t.t("noDescription")).small().dim().into()
        } else if body.chars().count() > BODY_SHOWN {
            pre(format!(
                "{}…",
                body.chars().take(BODY_SHOWN).collect::<String>()
            ))
            .height(320.0)
            .into()
        } else {
            pre(body.to_owned()).height(320.0).into()
        };
        section(t.t("description"), [content]).into()
    }

    fn pull_page(&self, data: &PullDetail, back: ui_button::Button) -> El {
        let t = &self.t;
        let mut state: Vec<El> = Vec::new();
        if data.checks.state != ChecksState::None {
            state.push(
                state_label(status_of(data.checks.state), self.checks_word(&data.checks)).into(),
            );
        }
        if let Some(word) = self.review_word(data.review) {
            state.push(text(word).small().dim().into());
        }
        if data.draft {
            state.push(text(t.t("draft")).small().dim().into());
        }
        let mut head = header(format!("#{} {}", data.number, data.title)).lead(back);
        for part in dotted(state) {
            head = head.state(part);
        }

        let mut children: Vec<El> = vec![head.into()];
        children.push(self.about(
            data.number,
            &data.url,
            vec![
                (t.t("author"), data.author.clone()),
                (t.t("branch"), format!("{} → {}", data.branch, data.base)),
                (
                    t.t("files"),
                    format!(
                        "{}, +{} −{}",
                        t.tn("fileCount", data.changed_files, &[]),
                        data.additions,
                        data.deletions
                    ),
                ),
                (t.t("labels"), data.labels.join(", ")),
            ],
        ));

        // Failing first, then running, then passing, then skipped.
        let rank = |state: CheckState| match state {
            CheckState::Failure => 0,
            CheckState::Running => 1,
            CheckState::Success => 2,
            CheckState::Neutral => 3,
        };
        let mut checks: Vec<&logic::CheckEntry> = data.check_list.iter().collect();
        checks.sort_by_key(|check| rank(check.state));
        if !checks.is_empty() {
            let rows = checks.iter().map(|check| {
                item(check.name.clone())
                    .lead(glyph(
                        status_of_check(check.state),
                        self.state_word(check.state),
                    ))
                    .wrap()
            });
            children.push(
                section(t.t("checks"), [list(rows).into()])
                    .count(checks.len().to_string())
                    .into(),
            );
        }

        if !data.reviews.is_empty() {
            let rows = data.reviews.iter().map(|review| {
                let word = match review.state.as_str() {
                    "APPROVED" => Some(t.t("reviewApproved")),
                    "CHANGES_REQUESTED" => Some(t.t("reviewChanges")),
                    _ => None,
                };
                let title = match word {
                    Some(word) => format!("{} · {word}", or_question(&review.author)),
                    None => or_question(&review.author),
                };
                let sub = excerpt(&review.body, 300);
                let row = item(title).wrap();
                if sub.is_empty() { row } else { row.sub(sub) }
            });
            children.push(
                section(t.t("reviews"), [list(rows).into()])
                    .count(data.reviews.len().to_string())
                    .into(),
            );
        }

        match &data.threads {
            None => children.push(
                section(
                    t.t("threads"),
                    [banner(Notice::Warning, t.t("threadsUnavailable")).into()],
                )
                .into(),
            ),
            Some(threads) => {
                let count = threads.unresolved.len();
                let content: El = if count == 0 {
                    text(t.t("noThreads")).small().dim().into()
                } else {
                    list(threads.unresolved.iter().map(|thread| {
                        let place = if thread.path.is_empty() {
                            String::new()
                        } else {
                            match thread.line.filter(|line| *line != 0) {
                                Some(line) => format!("{}:{line}", thread.path),
                                None => thread.path.clone(),
                            }
                        };
                        let title = if place.is_empty() {
                            or_question(&thread.author)
                        } else {
                            format!("{} · {place}", or_question(&thread.author))
                        };
                        let sub = excerpt(&thread.body, 300);
                        let row = item(title).wrap();
                        if sub.is_empty() { row } else { row.sub(sub) }
                    }))
                    .into()
                };
                children.push(
                    section(t.t("threads"), [content])
                        .count(count.to_string())
                        .into(),
                );
            }
        }

        if !data.files.is_empty() {
            let mut rows: Vec<ui_list::ListItem> = data
                .files
                .iter()
                .take(FILES_SHOWN)
                .map(|file| {
                    item(file.path.clone())
                        .mono()
                        .sub(format!("+{} −{}", file.additions, file.deletions))
                        .wrap()
                })
                .collect();
            if data.files.len() > FILES_SHOWN {
                rows.push(
                    item(t.tn("moreFiles", (data.files.len() - FILES_SHOWN) as i64, &[])).wrap(),
                );
            }
            children.push(
                section(t.t("files"), [list(rows).into()])
                    .count(data.files.len().to_string())
                    .into(),
            );
        }

        children.push(self.description(&data.body));
        page(children).into()
    }

    fn issue_page(&self, data: &IssueDetail, back: ui_button::Button) -> El {
        let t = &self.t;
        let head = header(format!("#{} {}", data.number, data.title))
            .lead(back)
            .state(text(data.state.to_lowercase()).small().dim());
        let mut children: Vec<El> = vec![head.into()];
        children.push(self.about(
            data.number,
            &data.url,
            vec![
                (t.t("author"), data.author.clone()),
                (t.t("assignees"), data.assignees.join(", ")),
                (t.t("labels"), data.labels.join(", ")),
            ],
        ));
        children.push(self.description(&data.body));
        if !data.comments.is_empty() {
            let skip = data.comments.len().saturating_sub(5);
            let rows = data.comments[skip..].iter().map(|comment| {
                let sub = excerpt(&comment.body, 400);
                let row = item(or_question(&comment.author)).wrap();
                if sub.is_empty() { row } else { row.sub(sub) }
            });
            children.push(
                section(t.t("comments"), [list(rows).into()])
                    .count(data.comments.len().to_string())
                    .into(),
            );
        }
        page(children).into()
    }
}

// The widgets' types, by what they are, for the signatures above.
mod ui_empty {
    pub type Empty = hangar_kit_sdk::ui::Empty<hangar_kit_sdk::Handler<crate::Message>>;
}
mod ui_list {
    pub type ListItem = hangar_kit_sdk::ui::ListItem<hangar_kit_sdk::Handler<crate::Message>>;
}
mod ui_button {
    pub type Button = hangar_kit_sdk::ui::Button<hangar_kit_sdk::Handler<crate::Message>>;
}
