# clickup-tui

Yazi-style terminal client for ClickUp. Read, edit, comment, and visualize your workspace without leaving the terminal. Rust + ratatui.

```
┌ Spaces ─────┐ ┌ Folders ──────┐ ┌ Lists ───────┐ ┌ Tasks ────────────┐ ┌ Detail ───────────┐
│ ▸ Sage      │ │ ▸ Operations  │ │ ▸ Active     │ │ ● Confirm staffing │ │ Title             │
│   SRC       │ │   Strategy    │ │   Backlog    │ │ ● Update menus     │ │ Confirm staffing  │
│   ...       │ │   ...         │ │   ...        │ │ ...                │ │ Status            │
└─────────────┘ └───────────────┘ └──────────────┘ └────────────────────┘ │ ● in progress     │
                                                                          │ Due Date          │
                                                                          │ Mar 13, 2026 (overdue)
                                                                          │ Priority          │
                                                                          │ ● urgent          │
                                                                          └───────────────────┘
```

## Install

```sh
cargo install --git https://github.com/shoot-here/clickup-tui
```

The binary is named `clickup` — run it from any terminal.

> Requires Rust ≥ 1.74. Install with `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` if needed.

## Configure

Drop a config at `~/.config/clickup-tui/config.toml`:

```toml
api_token = "pk_xxxxxxxxxxxxxxxxxxxxxxxxxx"
```

Generate a token at **ClickUp → Settings → Apps → Generate (Personal API Token)**. The token is stored locally in plaintext — treat the file like an SSH key.

## Features

**Navigation**
- 5-pane Yazi-style miller view: Spaces → Folders → Lists → Tasks → Detail
- Global search (`/`) across the whole workspace, with breadcrumb hits
- Multi-axis filters: assignee, status, priority, overdue, hide-empty-lists

**Editing**
- Title, description, status, priority, due date, assignees, comments — all in-app, no browser
- Native textarea modal for description (multi-line, with `Ctrl+Enter` save)

**Visualization**
- Top status bar shows live progress bar while the workspace loads
- Dashboard (`Up Arrow` from a top-row pane → `Enter`) with bar chart of open tasks by person and pie chart of tasks by status

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

## License

MIT
