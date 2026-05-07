# clickup-tui

Yazi-style terminal client for ClickUp. Read, edit, comment, and visualize your workspace without leaving the terminal. Rust + ratatui.

> _Independent open-source project. Not affiliated with, endorsed by, or sponsored by ClickUp Inc. ClickUp is a trademark of ClickUp Inc._

```
┌ Spaces ───┐ ┌ Folders ─────┐ ┌ Lists ────────┐ ┌ Tasks ──────────────┐ ┌ Detail ──────────┐
│ ▸ Work    │ │ ▸ Sprint 4   │ │ ▸ In Progress │ │ ● Implement search   │ │ Title            │
│   Side    │ │   Backlog    │ │   To Do       │ │ ● Fix login redirect │ │ Implement search │
│   Inbox   │ │   Roadmap    │ │   Done        │ │ ● Update changelog   │ │ Status           │
└───────────┘ └──────────────┘ └───────────────┘ └──────────────────────┘ │ ● in progress    │
                                                                          │ Due Date         │
                                                                          │ Mar 13 (overdue) │
                                                                          │ Priority         │
                                                                          │ ● urgent         │
                                                                          └──────────────────┘
```

## Install

```sh
cargo install --git https://github.com/shoot-here/clickup-tui
```

The binary is named `clickup` — run it from any terminal.

> Requires Rust ≥ 1.74. Install Rust with `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` if you don't have it.

## First launch

On first launch you'll see a welcome screen with a centered input box for your **ClickUp Personal API token**. Generate one at:

**ClickUp → Settings → Apps → Generate (Personal API Token)**

Paste it in, hit Enter, and you're in. The token is saved to `~/.config/clickup-tui/config.toml` with `chmod 600`. Treat that file like an SSH key.

If you ever need to switch tokens (or revoked your old one), press `Esc` from any pane → `Change API key`.

## Features

**Navigation**
- 5-pane Yazi-style miller view: Spaces → Folders → Lists → Tasks → Detail
- Global search (`/`) across the whole workspace, with breadcrumb hits
- Multi-axis filters: assignee, status, priority, overdue, hide-empty-lists

**Editing**
- Title, description, status, priority, due date, assignees, comments — all in-app, no browser
- Native textarea modal for description (multi-line, with `Ctrl+Enter` save)

**Visualization**
- Top status bar shows a live progress bar while the workspace loads
- Dashboard (`↑` from a top-row pane → `Enter`) with a bar chart of open tasks by person and a pie chart of tasks by status

## Keys

### Navigation
| Key | Action |
|-----|--------|
| `h` / `←` / `Shift+Tab` | Previous pane |
| `l` / `→` / `Tab` / `Enter` | Next pane (or activate detail field) |
| `j` / `↓` | Next item in pane |
| `k` / `↑` | Previous item in pane (`↑` at top row escalates to topbar callout) |
| `PgUp` / `PgDn` | Jump 10 items |
| `r` | Refresh focused pane |
| `Esc` | Open settings overlay (help + change API key) |
| `q` | Quit |

### Edit / create
| Key | Action |
|-----|--------|
| `t` | Edit task title |
| `e` | Edit description |
| `s` | Change status (color-coded picker) |
| `p` | Change priority (urgent / high / normal / low / clear) |
| `d` | Edit due date (`YYYY-MM-DD`, `today`, `tomorrow`, `+7d`, or `clear`) |
| `a` | Edit assignees (multi-select) |
| `c` | Add comment |
| `n` | Create new task in current list |

### Search / filter / dashboard
| Key | Action |
|-----|--------|
| `/` | Global search |
| `f` | Filter overlay |
| `↑` (top row, then Enter) | Open dashboard |

### Modal save
| Key | Action |
|-----|--------|
| `Ctrl+Enter` / `Alt+Enter` / `F2` | Save and exit modal |
| `Esc` | Cancel modal |

## Why a token, not OAuth?

ClickUp supports OAuth, but for a CLI tool it requires registering a developer app and embedding a client secret in the binary. Personal API tokens are one-click in ClickUp's settings, scoped to the same access as your account, and revocable independently of your password. For a local terminal client, that's the right tradeoff.

## Status

v0.1 — read + write across the surface area you'd expect from a daily-driver ClickUp client. Deferred:

- Subtasks tree-expand
- Custom fields editing (display works)
- Multi-workspace switcher (assumes one)
- Pagination — currently grabs the first 100 tasks per list

## License

MIT
