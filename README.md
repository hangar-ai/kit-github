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

## Native tabs

Both tabs are **native**: one WebAssembly module written in Rust (`tabs/github.wasm`,
from `native/`), drawn by Hangar itself in your theme, with no browser engine behind
it. They need Hangar 2.0 or later.

Text cannot be selected with the pointer (there are copy buttons for the link and for
the command that fixes `gh`), and "Hand to an agent" is a menu that asks which agents
there are when it opens, so it lists them a moment after the click.

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
module, and is a `query`, never a `mutation`.

Each tab holds `panes.read` and `panes.send` (to list the agents and send them the
brief) and `data.read` and `data.write` (its filters, in the kit's own data folder), and
the commands it uses: no files, no notification, no network of its own.

## Install

You need Hangar 2.0.0 or later and `gh` signed in (`gh auth login`).

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
├── tabs/github.wasm           the native tabs, built from native/ and committed: Hangar never builds a kit
├── native/                    one crate and one module for both tabs (init reads start.tab.id)
│   ├── src/logic.rs              gh's JSON → rows, filters, badge, the brief
│   ├── src/strings.rs            en and fr
│   ├── src/view.rs               the page as widgets, a function of the state
│   ├── src/lib.rs                the state, the messages, the calls to Hangar
│   └── tests/                    cargo test, on hangar-kit-sdk's harness; they read test/fixtures/
├── scripts/build-native.sh    native/ → tabs/github.wasm (--check: is the committed one current)
├── rust-toolchain.toml        the compiler the committed module is built with
├── test/                         manifest.test.js; fixtures/ are recorded gh output
└── package.json, run-tests.js    let `node --test kits/github/` find the tests on any Node
```

## Tests

```
cargo test --manifest-path native/Cargo.toml # the native tabs
node --test kits/github/                     # the manifest
```

`cargo test` runs the tabs on the harness of `hangar-kit-sdk` (the same runtime the
module runs, on the host, with Hangar played by a test) on recorded `gh` output,
including `gh` absent, not signed in and not a repository: the logic, what each tab asks
for and when, what the page shows, and that every call it makes is one the manifest
grants that tab. The fixtures use invented names.

`node --test` needs no install step. It checks the manifest against the rules of
[docs/kits.md](https://github.com/hangar-ai/hangar/blob/main/docs/kits.md) and
[docs/native/kits-native.md](https://github.com/hangar-ai/hangar/blob/main/docs/native/kits-native.md): `gh` only, reads
only, ids that cannot become an option, minimal permissions, and the native module the
tabs name.

## This repository

The tabs need Rust (the version `rust-toolchain.toml` names, with rustup) and the SDK,
which Cargo fetches from the `feat/native-rust` branch of
[hangar-ai/hangar](https://github.com/hangar-ai/hangar), locked by `native/Cargo.lock`
(a release tag replaces the branch once one holds the SDK).

**After changing anything in `native/`, run `scripts/build-native.sh` and commit
`tabs/github.wasm` with it.** The same sources and compiler give the same bytes on any
machine, and CI builds the module again and fails if the committed one differs
(`scripts/build-native.sh --check` does the same locally).

To try it in a window without Hangar:
`cargo run -p hangar-kit-host --example preview -- tabs/github.wasm` from the Hangar
repository. In a Hangar, link this folder in Settings › Kits and reload the tab from its
bar after each build.

CI calls [kit-check](https://github.com/hangar-ai/.github/blob/main/.github/workflows/kit-check.yml). A release is
a tag, `v` and the manifest's version, and it reaches Hangar's gallery once
[the catalog](https://github.com/hangar-ai/catalog) is updated and reviewed.
