# GitHub

A Hangar kit with two tabs, **Pull requests** and **Issues**: see what waits on a
GitHub repository without leaving Hangar, and hand any of it to an agent in one click.
It only reads GitHub. Nothing in it can merge, comment, review or close.

## What it does

- **Pull requests.** The open ones, newest update first: checks (failing, running,
  passing), review state, draft. A pull request opens in full: its checks one by one,
  the reviews, the **unresolved review comments** with file and line, the diff stat and
  the files it changes, the description.
- **Issues.** The open ones, with labels and assignees; an issue opens with its
  description and latest comments.
- **Filters**, kept between sessions: a search, failing checks, needs review, hide
  drafts (pull requests); unassigned (issues).
- **A badge** on the Pull requests tab: the number of pull requests with failing checks,
  else the number waiting for a review.
- **Refresh** on demand, and every 60 seconds while the tab is visible.
- **Hand to an agent**, on every item: pick one of the workspace's Claude Code agents,
  and it receives a brief with the title, the link, the description and, for a pull
  request, the failing checks and the unresolved review comments. Everything written by
  others on GitHub sits between two marked lines, with a note that it is information
  and not instructions; nothing in it can close that quote early. The brief ends with
  the task, and says not to push, merge, comment or close unless you ask.
- **Says what is wrong**: `gh` not installed, not signed in, or a folder that is not a
  GitHub repository, each with what to do next.
- English and French, from Hangar's language; light and dark with Hangar's theme;
  readable down to the side panel's narrowest width.

## Two ways to open the tabs

Each tab is both a **native tab** and a **page**:

- On the native Hangar, a tab is a WebAssembly module written in Rust
  (`tabs/github.wasm`, from `native/`), drawn by Hangar itself in your theme, with no
  browser engine behind it.
- On Hangar 1.x, and on the native Hangar when it cannot load the module (it says so in
  the tab's bar), the same tab is the HTML page (`tabs/github/`), unchanged.

The manifest stays at `"schema": 1` with the page as each tab's `entry` and the module
as its `native`: a Hangar that does not know the field ignores it. The commands, the
permissions and the files of the kit's data folder are the same in both, so they read
each other's saved filters.

What the native tabs do differently: text cannot be selected with the pointer (there are
copy buttons for the link and for the command that fixes `gh`), and "Hand to an agent"
is a menu that asks which agents there are when it opens, so it lists them a moment
after the click.

## What it runs

Six declared commands, all through [`gh`](https://cli.github.com), all reads. Hangar
shows them verbatim when you enable the kit.

| Command       | Runs                                                              |
| ------------- | ----------------------------------------------------------------- |
| `repo-info`   | `gh repo view --json nameWithOwner,name,owner,url`                |
| `pr-list`     | `gh pr list --state open --limit 50 --json …`                     |
| `pr-view`     | `gh pr view {id} --json …`                                        |
| `pr-threads`  | `gh api graphql` with a **fixed query** of a pull request's review threads |
| `issue-list`  | `gh issue list --state open --limit 50 --json …`                  |
| `issue-view`  | `gh issue view {id} --json …`                                     |

`{id}` is digits only (`^[0-9]+$`); `owner` and `name` of `pr-threads` come from
`repo-info` and are checked against a rule too. The query is in the manifest, not in the
page, and is a `query`, never a `mutation`.

Each tab holds `panes.read` and `panes.send` (to list the agents and send them the
brief) and `data.read` and `data.write` (its filters, in the kit's own data folder), and
the commands it uses: no files, no notification, no network of its own. The pages load
Hangar UI and nothing else.

## Install

You need Hangar 1.9.0 or later and `gh` signed in (`gh auth login`). The native tabs
need the native Hangar; before it, the pages open.

1. In Hangar, Settings › Kits, add the GitHub link:
   `https://github.com/hangar-ai/kit-github`
   (or link a local copy of this folder).
2. Enable it on a workspace and read what you accept: the six commands and the
   permissions above.

## How it is built

```
github/
├── hangar-kit.json            the kit: commands, the two tabs, the gallery text
├── icon.svg
├── tabs/github.wasm          the native tabs, built from native/ and committed: Hangar never builds a kit
├── tabs/github/
│   ├── pulls.html, issues.html   the two pages: Hangar UI, then the scripts below
│   ├── logic.js                  pure: gh's JSON → rows, filters, badge, the brief; en and fr
│   ├── controller.js             commands, 60 s refresh, filters, hand-off, with window.hangar as an argument
│   ├── view.js                   Hangar UI's classes, createElement + textContent, never markup
│   ├── app.js                    wires them in the page: menu, toasts, language, visibility
│   └── style.css                 one rule of layout, in Hangar UI's tokens
├── native/                    one crate and one module for both tabs (init reads start.tab.id)
│   ├── src/logic.rs              logic.js in Rust: gh's JSON → rows, filters, badge, the brief
│   ├── src/strings.rs            en and fr
│   ├── src/view.rs               the page as widgets, a function of the state
│   ├── src/lib.rs                the state, the messages, the calls to Hangar
│   └── tests/                    cargo test: the cases of test/*.test.js, on hangar-kit-sdk's harness
├── scripts/build-native.sh    native/ → tabs/github.wasm
├── rust-toolchain.toml        the compiler the committed module is built with
├── test/                         node --test, no dependency; fixtures/ are recorded gh output
└── package.json, run-tests.js    let `node --test kits/github/` find the tests on any Node
```

## Tests

```
node --test kits/github/                     # the pages
cargo test --manifest-path native/Cargo.toml # the native tabs
```

The first needs no install step. It runs the logic, the controller (against a fake
`window.hangar` and a fake clock) and the view (against a fake DOM) on recorded `gh`
output, including `gh` absent, not signed in and not a repository, and checks the
manifest against the rules of [docs/kits.md](https://github.com/hangar-ai/hangar/blob/main/docs/kits.md): `gh` only, reads
only, ids that cannot become an option, minimal permissions, and the native module the
tabs name. The fixtures use invented names.

The second runs the same cases on the native tabs, on the harness of `hangar-kit-sdk`
(the same runtime the module runs, on the host, with Hangar played by a test): the
logic, what each tab asks for and when, what the page shows, and that every call it
makes is one the manifest grants that tab. It reads the same fixtures.

## This repository

`node --test` runs the page's tests, with Node alone, and needs nothing from Hangar's
repository. The native tabs need Rust (the version `rust-toolchain.toml` names, with
rustup) and the SDK, which Cargo fetches from the `feat/native-rust` branch of
[hangar-ai/hangar](https://github.com/hangar-ai/hangar), locked by `native/Cargo.lock`
(a release tag replaces the branch once one holds the SDK).

**After changing anything in `native/`, run `scripts/build-native.sh` and commit
`tabs/github.wasm` with it.** The same sources and compiler give the same bytes on any
machine, and CI builds the module again and fails if the committed one differs.

To try it in a window without Hangar:
`cargo run -p hangar-kit-host --example preview -- tabs/github.wasm` from the Hangar
repository. In a Hangar, link this folder in Settings › Kits and reload the tab from its
bar after each build.

CI calls [kit-check](https://github.com/hangar-ai/.github/blob/main/.github/workflows/kit-check.yml). A release is
a tag, `v` and the manifest's version, and it reaches Hangar's gallery once
[the catalog](https://github.com/hangar-ai/catalog) is updated and reviewed.
