use crate::api::{
    ApiList, Client, Comment, ListEntry, MemberUser, Space, SpaceContents, Task, TaskStatus,
    Workspace, PRIORITY_OPTIONS,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Semaphore;
use tui_textarea::TextArea;

/// Cap on concurrent in-flight ClickUp HTTP requests. ClickUp's per-token
/// limit is ~100 req/min; 6 concurrent keeps us well below that.
const FETCH_CONCURRENCY: usize = 6;

/// Actions in the settings overlay. Index aligns with `activate_settings`.
pub const SETTINGS_ACTIONS: &[&str] = &["Change API key", "Quit"];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Spaces,
    Folders,
    Lists,
    Tasks,
    Detail,
}

impl Pane {
    pub fn index(self) -> usize {
        match self {
            Pane::Spaces => 0,
            Pane::Folders => 1,
            Pane::Lists => 2,
            Pane::Tasks => 3,
            Pane::Detail => 4,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Comment,
    EditTitle,
    EditDescription,
    EditDueDate,
    StatusPicker,
    PriorityPicker,
    Dashboard,
    Settings,
    Search,
    FilterOverlay,
    AssigneePicker,
    StatusFilterPicker,
    PriorityFilterPicker,
    AssigneeEditor,
    NewTask,
}

#[derive(Clone, Debug)]
pub struct FolderEntry {
    /// `None` = synthetic "(direct lists)" entry holding folderless lists
    pub id: Option<String>,
    pub name: String,
    pub list_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    Space,
    Folder,
    List,
    Task,
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub kind: HitKind,
    pub label: String,
    pub space_id: String,
    pub folder_id: Option<String>,
    pub list_id: Option<String>,
    pub task_id: Option<String>,
}

pub enum Message {
    WorkspacesLoaded(Vec<Workspace>),
    SpacesLoaded(Vec<Space>),
    SpaceContentsLoaded {
        space_id: String,
        contents: SpaceContents,
    },
    TasksLoaded {
        list_id: String,
        tasks: Vec<Task>,
    },
    TaskDetailLoaded {
        task: Task,
        comments: Vec<Comment>,
    },
    CommentPosted,
    StatusOptionsLoaded {
        list_id: String,
        statuses: Vec<TaskStatus>,
    },
    TaskUpdated,
    TaskCreated {
        list_id: String,
        task: Task,
    },
    Error(String),
}

pub struct App {
    client: Client,
    tx: UnboundedSender<Message>,
    fetch_sem: Arc<Semaphore>,

    pub workspace: Option<Workspace>,
    pub spaces: Vec<Space>,
    pub folders: Vec<FolderEntry>,
    pub lists: Vec<ListEntry>,
    pub tasks: Vec<Task>,
    pub task_detail: Option<Task>,
    pub comments: Vec<Comment>,

    pub spaces_state: ListState,
    pub folders_state: ListState,
    pub lists_state: ListState,
    pub tasks_state: ListState,
    pub detail_state: ListState,

    pub spaces_filter: String,
    pub folders_filter: String,
    pub lists_filter: String,
    pub tasks_filter: String,

    pub spaces_view: Vec<usize>,
    pub folders_view: Vec<usize>,
    pub lists_view: Vec<usize>,
    pub tasks_view: Vec<usize>,

    pub search_query: String,
    pub search_hits: Vec<SearchHit>,
    pub search_hits_state: ListState,

    bg_load_active: bool,
    bg_pending_spaces: HashSet<String>,
    bg_pending_lists: HashSet<String>,
    /// Cumulative count of fetches queued in the current background-load run.
    /// Used together with `bg_pending_count()` to render progress in the status bar.
    bg_total_spaces: usize,
    bg_total_lists: usize,

    /// `Some(name)` filters tasks to that assignee; literal `"(unassigned)"`
    /// matches tasks with no assignees.
    pub filter_assignee: Option<String>,
    pub filter_hide_empty: bool,
    pub filter_state: ListState,
    pub assignee_options: Vec<String>,
    pub assignee_picker_state: ListState,

    /// `Some(name)` filters tasks to that ClickUp status; `Some("(no status)")`
    /// matches tasks with no status; `None` is no filter.
    pub filter_status: Option<String>,
    /// `None` = no priority filter. `Some(Some(n))` = priority id n (1-4).
    /// `Some(None)` = tasks with no priority set.
    pub filter_priority: Option<Option<u8>>,
    pub filter_overdue: bool,
    pub status_filter_options: Vec<String>,
    pub status_filter_picker_state: ListState,
    pub priority_filter_picker_state: ListState,

    pub members: Vec<MemberUser>,
    pub assignee_editor_state: ListState,
    pub assignee_editor_selections: HashSet<i64>,

    pub new_task_buf: TextArea<'static>,

    space_contents_cache: HashMap<String, SpaceContents>,
    tasks_cache: HashMap<String, Vec<Task>>,

    pub focused: Pane,
    /// True when keyboard focus is on the top-bar task-count callout
    /// (escalated from a top-row pane via Up arrow).
    pub topbar_focused: bool,
    pub mode: Mode,
    pub comment_buf: TextArea<'static>,
    pub title_buf: TextArea<'static>,
    pub description_buf: TextArea<'static>,
    pub status_options: Vec<TaskStatus>,
    pub status_picker_state: ListState,
    status_options_for_list: Option<String>,
    pub priority_picker_state: ListState,
    pub due_date_buf: TextArea<'static>,
    pub due_date_error: Option<String>,
    pub status: String,
    pub should_quit: bool,
    /// When true alongside `should_quit`, main loop re-runs the welcome flow
    /// instead of exiting — used by the "Change API key" settings action.
    pub should_reauth: bool,
    pub settings_state: ListState,
}

impl App {
    pub fn new(client: Client, tx: UnboundedSender<Message>) -> Self {
        Self {
            client,
            tx,
            fetch_sem: Arc::new(Semaphore::new(FETCH_CONCURRENCY)),
            workspace: None,
            spaces: vec![],
            folders: vec![],
            lists: vec![],
            tasks: vec![],
            task_detail: None,
            comments: vec![],
            spaces_state: ListState::default(),
            folders_state: ListState::default(),
            lists_state: ListState::default(),
            tasks_state: ListState::default(),
            detail_state: {
                let mut s = ListState::default();
                s.select(Some(0));
                s
            },
            spaces_filter: String::new(),
            folders_filter: String::new(),
            lists_filter: String::new(),
            tasks_filter: String::new(),
            spaces_view: Vec::new(),
            folders_view: Vec::new(),
            lists_view: Vec::new(),
            tasks_view: Vec::new(),
            search_query: String::new(),
            search_hits: Vec::new(),
            search_hits_state: ListState::default(),
            bg_load_active: false,
            bg_pending_spaces: HashSet::new(),
            bg_pending_lists: HashSet::new(),
            bg_total_spaces: 0,
            bg_total_lists: 0,
            filter_assignee: None,
            filter_hide_empty: false,
            filter_state: ListState::default(),
            assignee_options: Vec::new(),
            assignee_picker_state: ListState::default(),
            filter_status: None,
            filter_priority: None,
            filter_overdue: false,
            status_filter_options: Vec::new(),
            status_filter_picker_state: ListState::default(),
            priority_filter_picker_state: ListState::default(),
            members: Vec::new(),
            assignee_editor_state: ListState::default(),
            assignee_editor_selections: HashSet::new(),
            new_task_buf: TextArea::default(),
            space_contents_cache: HashMap::new(),
            tasks_cache: HashMap::new(),
            focused: Pane::Spaces,
            topbar_focused: false,
            mode: Mode::Normal,
            comment_buf: TextArea::default(),
            title_buf: TextArea::default(),
            description_buf: TextArea::default(),
            status_options: vec![],
            status_picker_state: ListState::default(),
            status_options_for_list: None,
            priority_picker_state: ListState::default(),
            due_date_buf: TextArea::default(),
            due_date_error: None,
            status: "loading workspaces…".into(),
            should_quit: false,
            should_reauth: false,
            settings_state: {
                let mut s = ListState::default();
                s.select(Some(0));
                s
            },
        }
    }

    pub fn bootstrap(&self) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match client.workspaces().await {
                Ok(ws) => {
                    let _ = tx.send(Message::WorkspacesLoaded(ws));
                }
                Err(e) => {
                    let _ = tx.send(Message::Error(format!("workspaces: {e}")));
                }
            }
        });
    }

    pub fn update(&mut self, msg: Message) {
        match msg {
            Message::WorkspacesLoaded(ws) => {
                if let Some(first) = ws.into_iter().next() {
                    self.status = format!("workspace: {}", first.name);
                    self.members = first
                        .members
                        .iter()
                        .map(|m| m.user.clone())
                        .filter(|u| !u.username.is_empty())
                        .collect();
                    self.members.sort_by(|a, b| a.username.cmp(&b.username));
                    let id = first.id.clone();
                    self.workspace = Some(first);
                    self.fetch_spaces(id);
                } else {
                    self.status = "no workspaces found".into();
                }
            }
            Message::SpacesLoaded(spaces) => {
                self.spaces = spaces;
                self.rebuild_spaces_view();
                if !self.spaces_view.is_empty() && self.spaces_state.selected().is_none() {
                    self.spaces_state.select(Some(0));
                }
                self.refresh_folders_for_selection();
                // Warm the cache eagerly so the top progress bar starts moving
                // and search/filter feels instant once the user reaches for it.
                if !self.bg_load_active {
                    self.start_background_load();
                }
            }
            Message::SpaceContentsLoaded { space_id, contents } => {
                self.space_contents_cache.insert(space_id.clone(), contents.clone());
                self.bg_pending_spaces.remove(&space_id);
                if self.bg_load_active {
                    self.queue_task_fetches(&contents);
                }
                if self.current_space_id().as_deref() == Some(space_id.as_str()) {
                    self.apply_space_contents();
                }
                if self.mode == Mode::Search {
                    self.rebuild_search_hits();
                }
            }
            Message::TasksLoaded { list_id, tasks } => {
                self.tasks_cache.insert(list_id.clone(), tasks.clone());
                self.bg_pending_lists.remove(&list_id);
                if self.current_list_id().as_deref() == Some(list_id.as_str()) {
                    self.tasks = tasks;
                    self.rebuild_tasks_view();
                    self.tasks_state
                        .select(if self.tasks_view.is_empty() { None } else { Some(0) });
                    self.refresh_task_detail();
                }
                // Match counts depend on loaded tasks — refresh upstream views.
                if self.filter_hide_empty {
                    self.rebuild_lists_view();
                    self.rebuild_folders_view();
                }
                if self.mode == Mode::Search {
                    self.rebuild_search_hits();
                }
                if self.mode == Mode::AssigneePicker {
                    self.refresh_assignee_options();
                }
            }
            Message::TaskDetailLoaded { task, comments } => {
                self.task_detail = Some(task);
                self.comments = comments;
            }
            Message::CommentPosted => {
                self.status = "comment posted".into();
                self.refresh_task_detail();
            }
            Message::StatusOptionsLoaded { list_id, statuses } => {
                if self.current_list_id().as_deref() == Some(list_id.as_str()) {
                    self.status_options = statuses;
                    self.status_options_for_list = Some(list_id);
                    let current = self
                        .task_detail
                        .as_ref()
                        .and_then(|t| t.status.as_ref())
                        .map(|s| s.status.clone());
                    let idx = current
                        .and_then(|s| {
                            self.status_options
                                .iter()
                                .position(|o| o.status == s)
                        })
                        .unwrap_or(0);
                    if !self.status_options.is_empty() {
                        self.status_picker_state.select(Some(idx));
                    }
                }
            }
            Message::TaskUpdated => {
                self.status = "task updated".into();
                self.refresh_task_detail();
                if let Some(list_id) = self.current_list_id() {
                    self.tasks_cache.remove(&list_id);
                    self.fetch_tasks(list_id);
                }
            }
            Message::TaskCreated { list_id, task } => {
                self.status = format!("created: {}", task.name);
                self.tasks_cache
                    .entry(list_id.clone())
                    .or_default()
                    .insert(0, task.clone());
                if self.current_list_id().as_deref() == Some(list_id.as_str()) {
                    self.tasks.insert(0, task);
                    self.rebuild_tasks_view();
                    if let Some(view_idx) = self.tasks_view.iter().position(|&i| i == 0) {
                        self.tasks_state.select(Some(view_idx));
                        self.refresh_task_detail();
                    }
                    self.focused = Pane::Tasks;
                }
            }
            Message::Error(e) => {
                self.status = format!("error: {e}");
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match self.mode {
            Mode::Normal => self.key_normal(key),
            Mode::Comment => self.key_comment(key),
            Mode::EditTitle => self.key_edit_title(key),
            Mode::EditDescription => self.key_edit_description(key),
            Mode::EditDueDate => self.key_edit_due_date(key),
            Mode::StatusPicker => self.key_status_picker(key),
            Mode::PriorityPicker => self.key_priority_picker(key),
            Mode::Dashboard => self.key_dashboard(key),
            Mode::Settings => self.key_settings(key),
            Mode::Search => self.key_search(key),
            Mode::FilterOverlay => self.key_filter_overlay(key),
            Mode::AssigneePicker => self.key_assignee_picker(key),
            Mode::StatusFilterPicker => self.key_status_filter_picker(key),
            Mode::PriorityFilterPicker => self.key_priority_filter_picker(key),
            Mode::AssigneeEditor => self.key_assignee_editor(key),
            Mode::NewTask => self.key_new_task(key),
        }
    }

    fn key_normal(&mut self, key: KeyEvent) {
        if self.topbar_focused {
            match key.code {
                KeyCode::Esc
                | KeyCode::Down
                | KeyCode::Char('j')
                | KeyCode::Tab
                | KeyCode::BackTab => {
                    self.topbar_focused = false;
                }
                KeyCode::Enter => {
                    self.topbar_focused = false;
                    self.enter_dashboard();
                }
                KeyCode::Char('q') => self.should_quit = true,
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc => self.enter_settings(),
            KeyCode::Char('h') | KeyCode::Left | KeyCode::BackTab => self.focus_prev(),
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Tab => self.focus_next(),
            KeyCode::Enter => {
                if self.focused == Pane::Detail {
                    self.activate_detail_field();
                } else {
                    self.focus_next();
                }
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_in_pane(1),
            KeyCode::Up => {
                if self.try_escalate_topbar() {
                    return;
                }
                self.move_in_pane(-1);
            }
            KeyCode::Char('k') => self.move_in_pane(-1),
            KeyCode::PageDown => self.move_in_pane(10),
            KeyCode::PageUp => self.move_in_pane(-10),
            KeyCode::Char('r') => self.refresh_focused(),
            KeyCode::Char('c') => self.enter_comment_mode(),
            KeyCode::Char('t') => self.enter_title_edit(),
            KeyCode::Char('e') => self.enter_description_edit(),
            KeyCode::Char('s') => self.enter_status_picker(),
            KeyCode::Char('a') => self.enter_assignee_editor(),
            KeyCode::Char('p') => self.enter_priority_picker(),
            KeyCode::Char('d') => self.enter_due_date_edit(),
            KeyCode::Char('n') => self.enter_new_task(),
            KeyCode::Char('/') => self.enter_search(),
            KeyCode::Char('f') => self.enter_filter_overlay(),
            _ => {}
        }
    }

    /// Move focus to the top-bar callout when at the top of a top-row pane.
    /// Returns true if focus was escalated (caller should not also move).
    fn try_escalate_topbar(&mut self) -> bool {
        let in_top_row = matches!(
            self.focused,
            Pane::Spaces | Pane::Folders | Pane::Lists | Pane::Tasks
        );
        if !in_top_row {
            return false;
        }
        let idx = match self.focused {
            Pane::Spaces => self.spaces_state.selected(),
            Pane::Folders => self.folders_state.selected(),
            Pane::Lists => self.lists_state.selected(),
            Pane::Tasks => self.tasks_state.selected(),
            _ => None,
        };
        if idx.unwrap_or(0) == 0 {
            self.topbar_focused = true;
            return true;
        }
        false
    }

    fn key_comment(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.comment_buf = TextArea::default();
            }
            KeyCode::Enter
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.submit_comment();
            }
            KeyCode::F(2) => self.submit_comment(),
            _ => {
                self.comment_buf.input(key);
            }
        }
    }

    fn activate_detail_field(&mut self) {
        if self.task_detail.is_none() {
            return;
        }
        match self.detail_state.selected().unwrap_or(0) {
            0 => self.enter_title_edit(),
            1 => self.enter_status_picker(),
            2 => self.enter_assignee_editor(),
            3 => self.enter_due_date_edit(),
            4 => self.enter_priority_picker(),
            5 => self.enter_description_edit(),
            6 => self.enter_comment_mode(),
            7 => self.status = "custom field editing — coming soon".into(),
            _ => {}
        }
    }

    fn detail_row_count(&self) -> usize {
        let has_custom = self
            .task_detail
            .as_ref()
            .map(|t| !t.custom_fields.is_empty())
            .unwrap_or(false);
        if has_custom {
            8
        } else {
            7
        }
    }

    fn focus_next(&mut self) {
        self.focused = match self.focused {
            Pane::Spaces => Pane::Folders,
            Pane::Folders => Pane::Lists,
            Pane::Lists => Pane::Tasks,
            Pane::Tasks => Pane::Detail,
            Pane::Detail => Pane::Detail,
        };
    }

    fn focus_prev(&mut self) {
        self.focused = match self.focused {
            Pane::Spaces => Pane::Spaces,
            Pane::Folders => Pane::Spaces,
            Pane::Lists => Pane::Folders,
            Pane::Tasks => Pane::Lists,
            Pane::Detail => Pane::Tasks,
        };
    }

    fn move_in_pane(&mut self, delta: isize) {
        let len = match self.focused {
            Pane::Spaces => self.spaces_view.len(),
            Pane::Folders => self.folders_view.len(),
            Pane::Lists => self.lists_view.len(),
            Pane::Tasks => self.tasks_view.len(),
            Pane::Detail => self.detail_row_count(),
        };
        if len == 0 {
            return;
        }
        let state = match self.focused {
            Pane::Spaces => &mut self.spaces_state,
            Pane::Folders => &mut self.folders_state,
            Pane::Lists => &mut self.lists_state,
            Pane::Tasks => &mut self.tasks_state,
            Pane::Detail => &mut self.detail_state,
        };
        let cur = state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len as isize) as usize;
        state.select(Some(next));
        match self.focused {
            Pane::Spaces => self.refresh_folders_for_selection(),
            Pane::Folders => self.refresh_lists_for_selection(),
            Pane::Lists => self.refresh_tasks_for_selection(),
            Pane::Tasks => self.refresh_task_detail(),
            Pane::Detail => {}
        }
    }

    fn refresh_focused(&mut self) {
        match self.focused {
            Pane::Spaces => {
                if let Some(ws) = self.workspace.as_ref().map(|w| w.id.clone()) {
                    self.fetch_spaces(ws);
                }
            }
            Pane::Folders => {
                if let Some(id) = self.current_space_id() {
                    self.space_contents_cache.remove(&id);
                    self.fetch_space_contents(id);
                }
            }
            Pane::Lists => {
                self.refresh_lists_for_selection();
            }
            Pane::Tasks => {
                if let Some(id) = self.current_list_id() {
                    self.tasks_cache.remove(&id);
                    self.fetch_tasks(id);
                }
            }
            Pane::Detail => self.refresh_task_detail(),
        }
    }

    fn current_space_id(&self) -> Option<String> {
        self.spaces_state
            .selected()
            .and_then(|i| self.spaces_view.get(i))
            .and_then(|&j| self.spaces.get(j))
            .map(|s| s.id.clone())
    }

    fn current_folder_entry(&self) -> Option<&FolderEntry> {
        self.folders_state
            .selected()
            .and_then(|i| self.folders_view.get(i))
            .and_then(|&j| self.folders.get(j))
    }

    fn current_list_id(&self) -> Option<String> {
        self.lists_state
            .selected()
            .and_then(|i| self.lists_view.get(i))
            .and_then(|&j| self.lists.get(j))
            .map(|l| l.id.clone())
    }

    fn current_task_id(&self) -> Option<String> {
        self.tasks_state
            .selected()
            .and_then(|i| self.tasks_view.get(i))
            .and_then(|&j| self.tasks.get(j))
            .map(|t| t.id.clone())
    }

    fn refresh_folders_for_selection(&mut self) {
        let Some(space_id) = self.current_space_id() else {
            self.folders.clear();
            self.folders_state.select(None);
            self.lists.clear();
            self.lists_state.select(None);
            self.tasks.clear();
            self.tasks_state.select(None);
            return;
        };
        if self.space_contents_cache.contains_key(&space_id) {
            self.apply_space_contents();
        } else {
            self.folders.clear();
            self.folders_state.select(None);
            self.lists.clear();
            self.lists_state.select(None);
            self.tasks.clear();
            self.tasks_state.select(None);
            self.fetch_space_contents(space_id);
        }
    }

    fn apply_space_contents(&mut self) {
        let Some(space_id) = self.current_space_id() else {
            return;
        };
        let Some(contents) = self.space_contents_cache.get(&space_id).cloned() else {
            return;
        };

        let mut entries: Vec<FolderEntry> = contents
            .folders
            .iter()
            .map(|f| FolderEntry {
                id: Some(f.id.clone()),
                name: f.name.clone(),
                list_count: f.lists.len(),
            })
            .collect();
        if !contents.folderless.is_empty() {
            entries.insert(
                0,
                FolderEntry {
                    id: None,
                    name: "(direct lists)".into(),
                    list_count: contents.folderless.len(),
                },
            );
        }
        self.folders = entries;
        self.rebuild_folders_view();
        if !self.folders_view.is_empty() && self.folders_state.selected().is_none() {
            self.folders_state.select(Some(0));
        }
        self.refresh_lists_for_selection();
    }

    fn refresh_lists_for_selection(&mut self) {
        let Some(space_id) = self.current_space_id() else {
            self.lists.clear();
            self.lists_state.select(None);
            return;
        };
        let Some(contents) = self.space_contents_cache.get(&space_id) else {
            self.lists.clear();
            self.lists_state.select(None);
            return;
        };
        let folder_id = self.current_folder_entry().map(|f| f.id.clone());
        let lists: Vec<ListEntry> = match folder_id {
            Some(Some(fid)) => contents
                .folders
                .iter()
                .find(|f| f.id == fid)
                .map(|f| f.lists.iter().map(api_list_to_entry).collect())
                .unwrap_or_default(),
            Some(None) => contents.folderless.iter().map(api_list_to_entry).collect(),
            None => Vec::new(),
        };
        self.lists = lists;
        self.rebuild_lists_view();
        self.lists_state
            .select(if self.lists_view.is_empty() { None } else { Some(0) });
        self.refresh_tasks_for_selection();
    }

    fn refresh_tasks_for_selection(&mut self) {
        let Some(list_id) = self.current_list_id() else {
            self.tasks.clear();
            self.tasks_state.select(None);
            self.task_detail = None;
            self.comments.clear();
            return;
        };
        if let Some(cached) = self.tasks_cache.get(&list_id).cloned() {
            self.tasks = cached;
            self.rebuild_tasks_view();
            self.tasks_state
                .select(if self.tasks_view.is_empty() { None } else { Some(0) });
            self.refresh_task_detail();
        } else {
            self.tasks.clear();
            self.tasks_view.clear();
            self.tasks_state.select(None);
            self.fetch_tasks(list_id);
        }
    }

    fn refresh_task_detail(&mut self) {
        self.detail_state.select(Some(0));
        let Some(task_id) = self.current_task_id() else {
            self.task_detail = None;
            self.comments.clear();
            return;
        };
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let task_res = client.task(&task_id).await;
            let comments_res = client.comments(&task_id).await;
            match (task_res, comments_res) {
                (Ok(task), Ok(comments)) => {
                    let _ = tx.send(Message::TaskDetailLoaded { task, comments });
                }
                (Err(e), _) | (_, Err(e)) => {
                    let _ = tx.send(Message::Error(format!("task detail: {e}")));
                }
            }
        });
    }

    fn fetch_spaces(&self, workspace_id: String) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match client.spaces(&workspace_id).await {
                Ok(s) => {
                    let _ = tx.send(Message::SpacesLoaded(s));
                }
                Err(e) => {
                    let _ = tx.send(Message::Error(format!("spaces: {e}")));
                }
            }
        });
    }

    fn fetch_space_contents(&self, space_id: String) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        let sem = self.fetch_sem.clone();
        tokio::spawn(async move {
            let _permit = sem.acquire_owned().await;
            let id = space_id.clone();
            match client.space_contents(&id).await {
                Ok(contents) => {
                    let _ = tx.send(Message::SpaceContentsLoaded {
                        space_id: id,
                        contents,
                    });
                }
                Err(e) => {
                    let _ = tx.send(Message::Error(format!("space contents: {e}")));
                }
            }
        });
    }

    fn fetch_tasks(&self, list_id: String) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        let sem = self.fetch_sem.clone();
        tokio::spawn(async move {
            let _permit = sem.acquire_owned().await;
            let id = list_id.clone();
            match client.tasks(&id).await {
                Ok(tasks) => {
                    let _ = tx.send(Message::TasksLoaded { list_id: id, tasks });
                }
                Err(e) => {
                    let _ = tx.send(Message::Error(format!("tasks: {e}")));
                }
            }
        });
    }

    fn enter_comment_mode(&mut self) {
        if self.current_task_id().is_some() {
            self.mode = Mode::Comment;
            self.comment_buf = TextArea::default();
            self.comment_buf
                .set_placeholder_text("type a comment, Ctrl+S to send, Esc to cancel");
        }
    }

    fn submit_comment(&mut self) {
        let Some(task_id) = self.current_task_id() else {
            return;
        };
        let text = self.comment_buf.lines().join("\n");
        if text.trim().is_empty() {
            self.mode = Mode::Normal;
            self.comment_buf = TextArea::default();
            return;
        }
        self.mode = Mode::Normal;
        self.comment_buf = TextArea::default();
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match client.post_comment(&task_id, &text).await {
                Ok(()) => {
                    let _ = tx.send(Message::CommentPosted);
                }
                Err(e) => {
                    let _ = tx.send(Message::Error(format!("comment: {e}")));
                }
            }
        });
    }

    // ── Title edit ────────────────────────────────────────────────────

    fn enter_title_edit(&mut self) {
        let Some(task) = self.task_detail.as_ref() else {
            return;
        };
        self.mode = Mode::EditTitle;
        self.title_buf = TextArea::from([task.name.as_str()]);
        self.title_buf.set_placeholder_text("rename task — ⏎ save  esc cancel");
    }

    fn key_edit_title(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.title_buf = TextArea::default();
            }
            KeyCode::Enter => self.submit_title(),
            _ => {
                self.title_buf.input(key);
            }
        }
    }

    fn submit_title(&mut self) {
        let Some(task_id) = self.current_task_id() else {
            return;
        };
        let new_title = self.title_buf.lines().join(" ");
        let new_title = new_title.trim().to_string();
        self.mode = Mode::Normal;
        self.title_buf = TextArea::default();
        if new_title.is_empty() {
            return;
        }
        let body = serde_json::json!({ "name": new_title });
        self.spawn_update(task_id, body, "title");
    }

    // ── Description edit (in-TUI) ─────────────────────────────────────

    fn enter_description_edit(&mut self) {
        let Some(task) = self.task_detail.as_ref() else {
            return;
        };
        let current = task
            .description
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| task.text_content.clone().filter(|s| !s.is_empty()))
            .unwrap_or_default();
        self.mode = Mode::EditDescription;
        self.description_buf = if current.is_empty() {
            TextArea::default()
        } else {
            TextArea::from(current.lines())
        };
        self.description_buf
            .set_placeholder_text("describe the task — Ctrl+S to save  Esc to cancel");
    }

    fn key_edit_description(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.description_buf = TextArea::default();
            }
            KeyCode::Enter
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.submit_description();
            }
            KeyCode::F(2) => self.submit_description(),
            _ => {
                self.description_buf.input(key);
            }
        }
    }

    fn submit_description(&mut self) {
        let Some(task_id) = self.current_task_id() else {
            self.mode = Mode::Normal;
            self.description_buf = TextArea::default();
            return;
        };
        let new_description = self.description_buf.lines().join("\n");
        self.mode = Mode::Normal;
        self.description_buf = TextArea::default();
        let body = serde_json::json!({
            "description": new_description,
            "markdown_description": new_description,
        });
        self.spawn_update(task_id, body, "description");
    }

    // ── Status picker ─────────────────────────────────────────────────

    fn enter_status_picker(&mut self) {
        let Some(list_id) = self.current_list_id() else {
            return;
        };
        if self.task_detail.is_none() {
            return;
        }
        self.mode = Mode::StatusPicker;
        if self.status_options_for_list.as_deref() != Some(list_id.as_str()) {
            self.status_options.clear();
            self.status_picker_state.select(None);
            self.fetch_status_options(list_id);
        } else if !self.status_options.is_empty()
            && self.status_picker_state.selected().is_none()
        {
            self.status_picker_state.select(Some(0));
        }
    }

    fn key_status_picker(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Char('j') | KeyCode::Down => self.move_status_picker(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_status_picker(-1),
            KeyCode::Enter => self.submit_status(),
            _ => {}
        }
    }

    fn move_status_picker(&mut self, delta: isize) {
        let len = self.status_options.len();
        if len == 0 {
            return;
        }
        let cur = self.status_picker_state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len as isize) as usize;
        self.status_picker_state.select(Some(next));
    }

    fn submit_status(&mut self) {
        let Some(idx) = self.status_picker_state.selected() else {
            return;
        };
        let Some(option) = self.status_options.get(idx).cloned() else {
            return;
        };
        let Some(task_id) = self.current_task_id() else {
            return;
        };
        self.mode = Mode::Normal;
        let body = serde_json::json!({ "status": option.status });
        self.spawn_update(task_id, body, "status");
    }

    fn fetch_status_options(&self, list_id: String) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let id = list_id.clone();
            match client.list_statuses(&id).await {
                Ok(statuses) => {
                    let _ = tx.send(Message::StatusOptionsLoaded {
                        list_id: id,
                        statuses,
                    });
                }
                Err(e) => {
                    let _ = tx.send(Message::Error(format!("statuses: {e}")));
                }
            }
        });
    }

    // ── Priority picker ───────────────────────────────────────────────

    fn enter_priority_picker(&mut self) {
        if self.task_detail.is_none() || self.current_task_id().is_none() {
            return;
        }
        let current = self
            .task_detail
            .as_ref()
            .and_then(|t| t.priority.as_ref())
            .map(|p| p.priority.to_lowercase());
        let idx = current
            .as_deref()
            .and_then(|name| {
                PRIORITY_OPTIONS
                    .iter()
                    .position(|(label, _, _)| label.to_lowercase() == name)
            })
            .unwrap_or(2);
        self.priority_picker_state.select(Some(idx));
        self.mode = Mode::PriorityPicker;
    }

    fn key_priority_picker(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Char('j') | KeyCode::Down => self.move_priority_picker(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_priority_picker(-1),
            KeyCode::Enter => self.submit_priority(),
            _ => {}
        }
    }

    fn move_priority_picker(&mut self, delta: isize) {
        let len = PRIORITY_OPTIONS.len();
        let cur = self.priority_picker_state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len as isize) as usize;
        self.priority_picker_state.select(Some(next));
    }

    fn submit_priority(&mut self) {
        let Some(idx) = self.priority_picker_state.selected() else {
            return;
        };
        let Some((_, id, _)) = PRIORITY_OPTIONS.get(idx).copied() else {
            return;
        };
        let Some(task_id) = self.current_task_id() else {
            return;
        };
        self.mode = Mode::Normal;
        let body = match id {
            Some(n) => serde_json::json!({ "priority": n }),
            None => serde_json::json!({ "priority": serde_json::Value::Null }),
        };
        self.spawn_update(task_id, body, "priority");
    }

    // ── Dashboard ─────────────────────────────────────────────────────

    fn enter_dashboard(&mut self) {
        self.mode = Mode::Dashboard;
    }

    fn key_dashboard(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Normal,
            _ => {}
        }
    }

    pub fn total_open_tasks(&self) -> usize {
        self.tasks_cache.values().map(|v| v.len()).sum()
    }

    // ── Settings ──────────────────────────────────────────────────────

    fn enter_settings(&mut self) {
        self.settings_state.select(Some(0));
        self.mode = Mode::Settings;
    }

    fn key_settings(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Normal,
            KeyCode::Char('j') | KeyCode::Down => self.move_settings(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_settings(-1),
            KeyCode::Enter | KeyCode::Char(' ') => self.activate_settings(),
            _ => {}
        }
    }

    fn move_settings(&mut self, delta: isize) {
        let len = SETTINGS_ACTIONS.len() as isize;
        let cur = self.settings_state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len) as usize;
        self.settings_state.select(Some(next));
    }

    fn activate_settings(&mut self) {
        match self.settings_state.selected().unwrap_or(0) {
            0 => {
                // Change API key — wipe config and signal main loop to re-run welcome.
                if let Err(e) = crate::config::Config::delete() {
                    self.status = format!("delete config: {e}");
                    return;
                }
                self.should_quit = true;
                self.should_reauth = true;
            }
            1 => {
                self.should_quit = true;
            }
            _ => self.mode = Mode::Normal,
        }
    }

    pub fn task_counts_by_assignee(&self) -> Vec<(String, u64)> {
        let mut counts: HashMap<String, u64> = HashMap::new();
        for tasks in self.tasks_cache.values() {
            for t in tasks {
                if t.assignees.is_empty() {
                    *counts.entry("(unassigned)".into()).or_insert(0) += 1;
                } else {
                    for a in &t.assignees {
                        let name = if a.username.is_empty() {
                            format!("user-{}", a.id)
                        } else {
                            a.username.clone()
                        };
                        *counts.entry(name).or_insert(0) += 1;
                    }
                }
            }
        }
        let mut v: Vec<(String, u64)> = counts.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v
    }

    /// Returns `(status_name, count, hex_color)` triples sorted by count desc.
    /// Color preserves the first non-empty hex seen for that status name.
    pub fn task_counts_by_status(&self) -> Vec<(String, u64, Option<String>)> {
        let mut counts: HashMap<String, (u64, Option<String>)> = HashMap::new();
        for tasks in self.tasks_cache.values() {
            for t in tasks {
                let (key, color) = match t.status.as_ref() {
                    Some(s) => (s.status.clone(), s.color.clone()),
                    None => ("(none)".into(), None),
                };
                let entry = counts.entry(key).or_insert((0, None));
                entry.0 += 1;
                if entry.1.is_none() {
                    entry.1 = color;
                }
            }
        }
        let mut v: Vec<(String, u64, Option<String>)> = counts
            .into_iter()
            .map(|(k, (c, col))| (k, c, col))
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v
    }

    // ── Due date edit ─────────────────────────────────────────────────

    fn enter_due_date_edit(&mut self) {
        let Some(task) = self.task_detail.as_ref() else {
            return;
        };
        if self.current_task_id().is_none() {
            return;
        }
        let prefill = task
            .due_date
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| s.parse::<i64>().ok())
            .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        self.due_date_buf = if prefill.is_empty() {
            TextArea::default()
        } else {
            TextArea::from([prefill.as_str()])
        };
        self.due_date_buf
            .set_placeholder_text("YYYY-MM-DD · today · tomorrow · +7d · clear");
        self.due_date_error = None;
        self.mode = Mode::EditDueDate;
    }

    fn key_edit_due_date(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.due_date_buf = TextArea::default();
                self.due_date_error = None;
            }
            KeyCode::Enter => self.submit_due_date(),
            _ => {
                self.due_date_buf.input(key);
                self.due_date_error = None;
            }
        }
    }

    fn submit_due_date(&mut self) {
        let Some(task_id) = self.current_task_id() else {
            self.mode = Mode::Normal;
            self.due_date_buf = TextArea::default();
            return;
        };
        let raw = self.due_date_buf.lines().join(" ");
        match parse_due_date_input(&raw) {
            Ok(value) => {
                self.mode = Mode::Normal;
                self.due_date_buf = TextArea::default();
                self.due_date_error = None;
                let body = match value {
                    Some(ms) => serde_json::json!({
                        "due_date": ms,
                        "due_date_time": false,
                    }),
                    None => serde_json::json!({
                        "due_date": serde_json::Value::Null,
                        "due_date_time": false,
                    }),
                };
                self.spawn_update(task_id, body, "due date");
            }
            Err(e) => {
                self.due_date_error = Some(e);
            }
        }
    }

    fn spawn_update(&self, task_id: String, body: serde_json::Value, label: &'static str) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match client.update_task(&task_id, body).await {
                Ok(_) => {
                    let _ = tx.send(Message::TaskUpdated);
                }
                Err(e) => {
                    let _ = tx.send(Message::Error(format!("update {label}: {e}")));
                }
            }
        });
    }

    // ── Global search ─────────────────────────────────────────────────

    fn enter_search(&mut self) {
        self.search_query.clear();
        self.search_hits.clear();
        self.search_hits_state.select(None);
        self.mode = Mode::Search;
        self.start_background_load();
    }

    fn start_background_load(&mut self) {
        // Reset totals when previous run is fully drained, otherwise keep accumulating.
        if self.bg_pending_count() == 0 {
            self.bg_total_spaces = 0;
            self.bg_total_lists = 0;
        }
        self.bg_load_active = true;
        // Phase 1: fetch contents for any unloaded spaces
        let unloaded: Vec<String> = self
            .spaces
            .iter()
            .filter(|s| !self.space_contents_cache.contains_key(&s.id))
            .map(|s| s.id.clone())
            .collect();
        for space_id in unloaded {
            if self.bg_pending_spaces.insert(space_id.clone()) {
                self.bg_total_spaces += 1;
                self.fetch_space_contents(space_id);
            }
        }
        // Phase 2: queue task fetches for already-cached spaces
        let cached: Vec<SpaceContents> = self.space_contents_cache.values().cloned().collect();
        for contents in cached {
            self.queue_task_fetches(&contents);
        }
    }

    fn queue_task_fetches(&mut self, contents: &SpaceContents) {
        let mut to_fetch: Vec<String> = Vec::new();
        for folder in &contents.folders {
            for list in &folder.lists {
                if !self.tasks_cache.contains_key(&list.id)
                    && self.bg_pending_lists.insert(list.id.clone())
                {
                    self.bg_total_lists += 1;
                    to_fetch.push(list.id.clone());
                }
            }
        }
        for list in &contents.folderless {
            if !self.tasks_cache.contains_key(&list.id)
                && self.bg_pending_lists.insert(list.id.clone())
            {
                self.bg_total_lists += 1;
                to_fetch.push(list.id.clone());
            }
        }
        for list_id in to_fetch {
            self.fetch_tasks(list_id);
        }
    }

    fn key_search(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.search_query.clear();
                self.search_hits.clear();
                self.search_hits_state.select(None);
            }
            KeyCode::Enter => self.drill_to_selected_hit(),
            KeyCode::Backspace => {
                self.search_query.pop();
                self.rebuild_search_hits();
            }
            KeyCode::Down => self.move_search_hits(1),
            KeyCode::Up => self.move_search_hits(-1),
            KeyCode::Char(c) => {
                self.search_query.push(c);
                self.rebuild_search_hits();
            }
            _ => {}
        }
    }

    fn move_search_hits(&mut self, delta: isize) {
        let len = self.search_hits.len();
        if len == 0 {
            return;
        }
        let cur = self.search_hits_state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len as isize) as usize;
        self.search_hits_state.select(Some(next));
    }

    fn rebuild_search_hits(&mut self) {
        let needle = self.search_query.to_lowercase();
        if needle.is_empty() {
            self.search_hits.clear();
            self.search_hits_state.select(None);
            return;
        }
        let mut hits: Vec<SearchHit> = Vec::new();

        // Spaces
        for space in &self.spaces {
            if space.name.to_lowercase().contains(&needle) {
                hits.push(SearchHit {
                    kind: HitKind::Space,
                    label: format!("Space   {}", space.name),
                    space_id: space.id.clone(),
                    folder_id: None,
                    list_id: None,
                    task_id: None,
                });
            }
        }

        // Folders + lists across cached space contents
        for (space_id, contents) in &self.space_contents_cache {
            let space_name = self
                .spaces
                .iter()
                .find(|s| &s.id == space_id)
                .map(|s| s.name.clone())
                .unwrap_or_default();
            for folder in &contents.folders {
                if folder.name.to_lowercase().contains(&needle) {
                    hits.push(SearchHit {
                        kind: HitKind::Folder,
                        label: format!("Folder  {space_name} › {}", folder.name),
                        space_id: space_id.clone(),
                        folder_id: Some(folder.id.clone()),
                        list_id: None,
                        task_id: None,
                    });
                }
                for list in &folder.lists {
                    if list.name.to_lowercase().contains(&needle) {
                        hits.push(SearchHit {
                            kind: HitKind::List,
                            label: format!(
                                "List    {space_name} › {} › {}",
                                folder.name, list.name
                            ),
                            space_id: space_id.clone(),
                            folder_id: Some(folder.id.clone()),
                            list_id: Some(list.id.clone()),
                            task_id: None,
                        });
                    }
                }
            }
            for list in &contents.folderless {
                if list.name.to_lowercase().contains(&needle) {
                    hits.push(SearchHit {
                        kind: HitKind::List,
                        label: format!("List    {space_name} › {}", list.name),
                        space_id: space_id.clone(),
                        folder_id: None,
                        list_id: Some(list.id.clone()),
                        task_id: None,
                    });
                }
            }
        }

        // Tasks across cached lists
        for (list_id, tasks) in &self.tasks_cache {
            let breadcrumb = self.breadcrumb_for_list(list_id);
            let Some((space_id, folder_id, list_name)) = breadcrumb else {
                continue;
            };
            for task in tasks {
                if task.name.to_lowercase().contains(&needle) {
                    let space_name = self
                        .spaces
                        .iter()
                        .find(|s| s.id == space_id)
                        .map(|s| s.name.clone())
                        .unwrap_or_default();
                    let crumb = match &folder_id {
                        Some(fid) => {
                            let folder_name = self
                                .space_contents_cache
                                .get(&space_id)
                                .and_then(|c| c.folders.iter().find(|f| &f.id == fid))
                                .map(|f| f.name.clone())
                                .unwrap_or_default();
                            format!("{space_name} › {folder_name} › {list_name}")
                        }
                        None => format!("{space_name} › {list_name}"),
                    };
                    hits.push(SearchHit {
                        kind: HitKind::Task,
                        label: format!("Task    {crumb} › {}", task.name),
                        space_id: space_id.clone(),
                        folder_id: folder_id.clone(),
                        list_id: Some(list_id.clone()),
                        task_id: Some(task.id.clone()),
                    });
                }
            }
        }

        self.search_hits = hits;
        if !self.search_hits.is_empty() {
            self.search_hits_state.select(Some(0));
        } else {
            self.search_hits_state.select(None);
        }
    }

    pub fn bg_pending_count(&self) -> usize {
        self.bg_pending_spaces.len() + self.bg_pending_lists.len()
    }

    /// Returns `(done, total)` for the active background load run, or `None`
    /// when there's nothing to show. Drives the progress bar in the status line.
    pub fn bg_progress(&self) -> Option<(usize, usize)> {
        let total = self.bg_total_spaces + self.bg_total_lists;
        if total == 0 {
            return None;
        }
        let pending = self.bg_pending_count();
        if pending == 0 {
            return None;
        }
        Some((total - pending, total))
    }

    fn breadcrumb_for_list(&self, list_id: &str) -> Option<(String, Option<String>, String)> {
        for (sid, contents) in &self.space_contents_cache {
            for folder in &contents.folders {
                if let Some(list) = folder.lists.iter().find(|l| l.id == list_id) {
                    return Some((sid.clone(), Some(folder.id.clone()), list.name.clone()));
                }
            }
            if let Some(list) = contents.folderless.iter().find(|l| l.id == list_id) {
                return Some((sid.clone(), None, list.name.clone()));
            }
        }
        None
    }

    fn drill_to_selected_hit(&mut self) {
        let Some(idx) = self.search_hits_state.selected() else {
            return;
        };
        let Some(hit) = self.search_hits.get(idx).cloned() else {
            return;
        };
        self.mode = Mode::Normal;
        self.search_query.clear();
        self.search_hits.clear();
        self.search_hits_state.select(None);
        self.drill_to_hit(hit);
    }

    fn drill_to_hit(&mut self, hit: SearchHit) {
        // Clear any prior pane filters so the hit's items are visible.
        self.spaces_filter.clear();
        self.folders_filter.clear();
        self.lists_filter.clear();
        self.tasks_filter.clear();

        // 1. Space
        let Some(space_data_idx) = self.spaces.iter().position(|s| s.id == hit.space_id) else {
            return;
        };
        self.rebuild_spaces_view();
        if let Some(view_idx) = self
            .spaces_view
            .iter()
            .position(|&i| i == space_data_idx)
        {
            self.spaces_state.select(Some(view_idx));
        }

        let Some(contents) = self.space_contents_cache.get(&hit.space_id).cloned() else {
            self.fetch_space_contents(hit.space_id.clone());
            self.focused = Pane::Spaces;
            return;
        };

        // 2. Folders pane
        let mut entries: Vec<FolderEntry> = contents
            .folders
            .iter()
            .map(|f| FolderEntry {
                id: Some(f.id.clone()),
                name: f.name.clone(),
                list_count: f.lists.len(),
            })
            .collect();
        if !contents.folderless.is_empty() {
            entries.insert(
                0,
                FolderEntry {
                    id: None,
                    name: "(direct lists)".into(),
                    list_count: contents.folderless.len(),
                },
            );
        }
        self.folders = entries;
        self.rebuild_folders_view();
        let target_folder_id = hit.folder_id.clone();
        if let Some(data_idx) = self
            .folders
            .iter()
            .position(|f| f.id == target_folder_id)
        {
            if let Some(view_idx) = self.folders_view.iter().position(|&i| i == data_idx) {
                self.folders_state.select(Some(view_idx));
            }
        }

        // 3. Lists pane
        let lists: Vec<ListEntry> = match &target_folder_id {
            Some(fid) => contents
                .folders
                .iter()
                .find(|f| f.id == *fid)
                .map(|f| f.lists.iter().map(api_list_to_entry).collect())
                .unwrap_or_default(),
            None => contents.folderless.iter().map(api_list_to_entry).collect(),
        };
        self.lists = lists;
        self.rebuild_lists_view();
        if let Some(target_list_id) = hit.list_id.as_deref() {
            if let Some(data_idx) = self.lists.iter().position(|l| l.id == target_list_id) {
                if let Some(view_idx) = self.lists_view.iter().position(|&i| i == data_idx) {
                    self.lists_state.select(Some(view_idx));
                }
            }
        }

        // 4. Tasks pane (if cached)
        if let Some(target_list_id) = hit.list_id.as_deref() {
            if let Some(cached) = self.tasks_cache.get(target_list_id).cloned() {
                self.tasks = cached;
                self.rebuild_tasks_view();
                if let Some(target_task_id) = hit.task_id.as_deref() {
                    if let Some(data_idx) =
                        self.tasks.iter().position(|t| t.id == target_task_id)
                    {
                        if let Some(view_idx) =
                            self.tasks_view.iter().position(|&i| i == data_idx)
                        {
                            self.tasks_state.select(Some(view_idx));
                        }
                    }
                }
                self.refresh_task_detail();
            } else {
                self.fetch_tasks(target_list_id.to_string());
            }
        }

        // 5. Focus the appropriate pane
        self.focused = match hit.kind {
            HitKind::Space => Pane::Spaces,
            HitKind::Folder => Pane::Folders,
            HitKind::List => Pane::Lists,
            HitKind::Task => Pane::Tasks,
        };
    }

    // ── View rebuilds ─────────────────────────────────────────────────

    fn rebuild_spaces_view(&mut self) {
        let needle = self.spaces_filter.to_lowercase();
        self.spaces_view = self
            .spaces
            .iter()
            .enumerate()
            .filter(|(_, s)| needle.is_empty() || s.name.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect();
        clamp(&mut self.spaces_state, self.spaces_view.len());
    }

    fn rebuild_folders_view(&mut self) {
        let needle = self.folders_filter.to_lowercase();
        let hide_empty = self.filter_hide_empty;
        let assignee = self.filter_assignee.clone();
        let space_id = self.current_space_id();
        let counts: Vec<usize> = if hide_empty && space_id.is_some() {
            let sid = space_id.as_deref().unwrap();
            self.folders
                .iter()
                .map(|fe| {
                    folder_match_count(
                        fe,
                        sid,
                        &self.space_contents_cache,
                        &self.tasks_cache,
                        assignee.as_deref(),
                    )
                })
                .collect()
        } else {
            vec![1; self.folders.len()]
        };
        self.folders_view = self
            .folders
            .iter()
            .enumerate()
            .filter(|(_, f)| needle.is_empty() || f.name.to_lowercase().contains(&needle))
            .filter(|(i, _)| !hide_empty || counts.get(*i).copied().unwrap_or(0) > 0)
            .map(|(i, _)| i)
            .collect();
        clamp(&mut self.folders_state, self.folders_view.len());
    }

    fn rebuild_lists_view(&mut self) {
        let needle = self.lists_filter.to_lowercase();
        let hide_empty = self.filter_hide_empty;
        let assignee = self.filter_assignee.clone();
        let counts: Vec<usize> = if hide_empty {
            self.lists
                .iter()
                .map(|l| {
                    list_match_count(&l.id, l.task_count, &self.tasks_cache, assignee.as_deref())
                })
                .collect()
        } else {
            vec![1; self.lists.len()]
        };
        self.lists_view = self
            .lists
            .iter()
            .enumerate()
            .filter(|(_, l)| needle.is_empty() || l.name.to_lowercase().contains(&needle))
            .filter(|(i, _)| !hide_empty || counts.get(*i).copied().unwrap_or(0) > 0)
            .map(|(i, _)| i)
            .collect();
        clamp(&mut self.lists_state, self.lists_view.len());
    }

    fn rebuild_tasks_view(&mut self) {
        let needle = self.tasks_filter.to_lowercase();
        let assignee = self.filter_assignee.clone();
        let status = self.filter_status.clone();
        let priority = self.filter_priority;
        let overdue = self.filter_overdue;
        let now_ms = chrono::Utc::now().timestamp_millis();
        self.tasks_view = self
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| needle.is_empty() || t.name.to_lowercase().contains(&needle))
            .filter(|(_, t)| match assignee.as_deref() {
                None => true,
                Some("(unassigned)") => t.assignees.is_empty(),
                Some(name) => t
                    .assignees
                    .iter()
                    .any(|a| a.username.eq_ignore_ascii_case(name)),
            })
            .filter(|(_, t)| match status.as_deref() {
                None => true,
                Some("(no status)") => t.status.is_none(),
                Some(name) => t
                    .status
                    .as_ref()
                    .map(|s| s.status.eq_ignore_ascii_case(name))
                    .unwrap_or(false),
            })
            .filter(|(_, t)| match priority {
                None => true,
                Some(None) => t.priority.is_none(),
                Some(Some(n)) => t
                    .priority
                    .as_ref()
                    .and_then(|p| priority_id_for_name(&p.priority))
                    .map(|id| id == n)
                    .unwrap_or(false),
            })
            .filter(|(_, t)| !overdue || is_overdue(t, now_ms))
            .map(|(i, _)| i)
            .collect();
        clamp(&mut self.tasks_state, self.tasks_view.len());
    }

    // ── Filters ───────────────────────────────────────────────────────

    fn enter_filter_overlay(&mut self) {
        self.mode = Mode::FilterOverlay;
        if self.filter_state.selected().is_none() {
            self.filter_state.select(Some(0));
        }
        // Make sure the workspace finishes loading so the assignee picker
        // and hide_empty filter can see everything.
        if !self.bg_load_active {
            self.start_background_load();
        }
    }

    fn key_filter_overlay(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('f') => self.mode = Mode::Normal,
            KeyCode::Char('j') | KeyCode::Down => self.move_filter_overlay(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_filter_overlay(-1),
            KeyCode::Char(' ') | KeyCode::Enter => self.activate_filter_row(),
            _ => {}
        }
    }

    fn move_filter_overlay(&mut self, delta: isize) {
        let len = 6;
        let cur = self.filter_state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len) as usize;
        self.filter_state.select(Some(next));
    }

    fn activate_filter_row(&mut self) {
        match self.filter_state.selected().unwrap_or(0) {
            0 => self.enter_assignee_picker(),
            1 => self.enter_status_filter_picker(),
            2 => self.enter_priority_filter_picker(),
            3 => {
                self.filter_overdue = !self.filter_overdue;
                self.rebuild_tasks_view();
            }
            4 => {
                self.filter_hide_empty = !self.filter_hide_empty;
                self.rebuild_lists_view();
                self.rebuild_folders_view();
            }
            5 => {
                self.filter_assignee = None;
                self.filter_status = None;
                self.filter_priority = None;
                self.filter_overdue = false;
                self.filter_hide_empty = false;
                self.rebuild_tasks_view();
                self.rebuild_lists_view();
                self.rebuild_folders_view();
            }
            _ => {}
        }
    }

    // ── Status filter picker ──────────────────────────────────────────

    fn enter_status_filter_picker(&mut self) {
        if !self.bg_load_active {
            self.start_background_load();
        }
        self.refresh_status_filter_options();
        let initial = match &self.filter_status {
            None => 0,
            Some(name) => self
                .status_filter_options
                .iter()
                .position(|o| o == name)
                .unwrap_or(0),
        };
        self.status_filter_picker_state.select(Some(initial));
        self.mode = Mode::StatusFilterPicker;
    }

    fn refresh_status_filter_options(&mut self) {
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for tasks in self.tasks_cache.values() {
            for t in tasks {
                if let Some(s) = &t.status {
                    if !s.status.is_empty() {
                        set.insert(s.status.clone());
                    }
                }
            }
        }
        let mut options = vec!["(any)".to_string(), "(no status)".to_string()];
        options.extend(set);
        let prior = self
            .status_filter_picker_state
            .selected()
            .and_then(|i| self.status_filter_options.get(i).cloned());
        self.status_filter_options = options;
        if let Some(prior_name) = prior {
            if let Some(new_idx) = self
                .status_filter_options
                .iter()
                .position(|o| o == &prior_name)
            {
                self.status_filter_picker_state.select(Some(new_idx));
            }
        }
    }

    fn key_status_filter_picker(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::FilterOverlay,
            KeyCode::Char('j') | KeyCode::Down => self.move_status_filter_picker(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_status_filter_picker(-1),
            KeyCode::Enter => self.apply_selected_status_filter(),
            _ => {}
        }
    }

    fn move_status_filter_picker(&mut self, delta: isize) {
        let len = self.status_filter_options.len();
        if len == 0 {
            return;
        }
        let cur = self.status_filter_picker_state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len as isize) as usize;
        self.status_filter_picker_state.select(Some(next));
    }

    fn apply_selected_status_filter(&mut self) {
        let Some(idx) = self.status_filter_picker_state.selected() else {
            return;
        };
        let Some(option) = self.status_filter_options.get(idx).cloned() else {
            return;
        };
        self.filter_status = if option == "(any)" { None } else { Some(option) };
        self.rebuild_tasks_view();
        self.mode = Mode::FilterOverlay;
    }

    // ── Priority filter picker ────────────────────────────────────────

    fn enter_priority_filter_picker(&mut self) {
        // 6 options: (any), Urgent, High, Normal, Low, (no priority)
        let initial = match self.filter_priority {
            None => 0,
            Some(Some(1)) => 1,
            Some(Some(2)) => 2,
            Some(Some(3)) => 3,
            Some(Some(4)) => 4,
            Some(None) => 5,
            _ => 0,
        };
        self.priority_filter_picker_state.select(Some(initial));
        self.mode = Mode::PriorityFilterPicker;
    }

    fn key_priority_filter_picker(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::FilterOverlay,
            KeyCode::Char('j') | KeyCode::Down => self.move_priority_filter_picker(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_priority_filter_picker(-1),
            KeyCode::Enter => self.apply_selected_priority_filter(),
            _ => {}
        }
    }

    fn move_priority_filter_picker(&mut self, delta: isize) {
        let len = 6;
        let cur = self.priority_filter_picker_state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len) as usize;
        self.priority_filter_picker_state.select(Some(next));
    }

    fn apply_selected_priority_filter(&mut self) {
        let idx = self.priority_filter_picker_state.selected().unwrap_or(0);
        self.filter_priority = match idx {
            0 => None,
            1 => Some(Some(1)),
            2 => Some(Some(2)),
            3 => Some(Some(3)),
            4 => Some(Some(4)),
            5 => Some(None),
            _ => None,
        };
        self.rebuild_tasks_view();
        self.mode = Mode::FilterOverlay;
    }

    fn enter_assignee_picker(&mut self) {
        if !self.bg_load_active {
            self.start_background_load();
        }
        self.refresh_assignee_options();
        let initial = match &self.filter_assignee {
            None => 0,
            Some(name) => self
                .assignee_options
                .iter()
                .position(|o| o == name)
                .unwrap_or(0),
        };
        self.assignee_picker_state.select(Some(initial));
        self.mode = Mode::AssigneePicker;
    }

    /// Workspace-wide pool of assignees so the picker is always complete.
    /// Re-run as background loads stream in to keep the picker live.
    fn refresh_assignee_options(&mut self) {
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for tasks in self.tasks_cache.values() {
            for t in tasks {
                for a in &t.assignees {
                    if !a.username.is_empty() {
                        set.insert(a.username.clone());
                    }
                }
            }
        }
        let mut options = vec!["(any)".to_string(), "(unassigned)".to_string()];
        options.extend(set);
        // Preserve the previously-selected option's identity if possible.
        let prior = self
            .assignee_picker_state
            .selected()
            .and_then(|i| self.assignee_options.get(i).cloned());
        self.assignee_options = options;
        if let Some(prior_name) = prior {
            if let Some(new_idx) = self
                .assignee_options
                .iter()
                .position(|o| o == &prior_name)
            {
                self.assignee_picker_state.select(Some(new_idx));
            }
        }
    }

    fn key_assignee_picker(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::FilterOverlay,
            KeyCode::Char('j') | KeyCode::Down => self.move_assignee_picker(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_assignee_picker(-1),
            KeyCode::Enter => self.apply_selected_assignee(),
            _ => {}
        }
    }

    fn move_assignee_picker(&mut self, delta: isize) {
        let len = self.assignee_options.len();
        if len == 0 {
            return;
        }
        let cur = self.assignee_picker_state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len as isize) as usize;
        self.assignee_picker_state.select(Some(next));
    }

    fn apply_selected_assignee(&mut self) {
        let Some(idx) = self.assignee_picker_state.selected() else {
            return;
        };
        let Some(option) = self.assignee_options.get(idx).cloned() else {
            return;
        };
        self.filter_assignee = if option == "(any)" { None } else { Some(option) };
        self.rebuild_tasks_view();
        self.rebuild_lists_view();
        self.rebuild_folders_view();
        self.mode = Mode::FilterOverlay;
    }

    // ── Assignee editor (multi-select) ────────────────────────────────

    fn enter_assignee_editor(&mut self) {
        let Some(task) = self.task_detail.as_ref() else {
            return;
        };
        if self.members.is_empty() {
            self.status = "no workspace members loaded yet".into();
            return;
        }
        self.assignee_editor_selections = task.assignees.iter().map(|a| a.id).collect();
        self.assignee_editor_state.select(Some(0));
        self.mode = Mode::AssigneeEditor;
    }

    fn key_assignee_editor(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.assignee_editor_selections.clear();
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_assignee_editor(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_assignee_editor(-1),
            KeyCode::Char(' ') => self.toggle_assignee_editor_row(),
            KeyCode::Enter => self.submit_assignees(),
            _ => {}
        }
    }

    fn move_assignee_editor(&mut self, delta: isize) {
        let len = self.members.len();
        if len == 0 {
            return;
        }
        let cur = self.assignee_editor_state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len as isize) as usize;
        self.assignee_editor_state.select(Some(next));
    }

    fn toggle_assignee_editor_row(&mut self) {
        let Some(idx) = self.assignee_editor_state.selected() else {
            return;
        };
        let Some(member) = self.members.get(idx) else {
            return;
        };
        if !self.assignee_editor_selections.remove(&member.id) {
            self.assignee_editor_selections.insert(member.id);
        }
    }

    // ── New task ──────────────────────────────────────────────────────

    fn enter_new_task(&mut self) {
        if self.current_list_id().is_none() {
            self.status = "select a list first to create a task".into();
            return;
        }
        self.mode = Mode::NewTask;
        self.new_task_buf = TextArea::default();
        self.new_task_buf
            .set_placeholder_text("new task name — ⏎ create  esc cancel");
    }

    fn key_new_task(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.new_task_buf = TextArea::default();
            }
            KeyCode::Enter => self.submit_new_task(),
            _ => {
                self.new_task_buf.input(key);
            }
        }
    }

    fn submit_new_task(&mut self) {
        let Some(list_id) = self.current_list_id() else {
            self.mode = Mode::Normal;
            self.new_task_buf = TextArea::default();
            return;
        };
        let name = self.new_task_buf.lines().join(" ").trim().to_string();
        self.mode = Mode::Normal;
        self.new_task_buf = TextArea::default();
        if name.is_empty() {
            return;
        }
        let body = serde_json::json!({ "name": name });
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match client.create_task(&list_id, body).await {
                Ok(task) => {
                    let _ = tx.send(Message::TaskCreated { list_id, task });
                }
                Err(e) => {
                    let _ = tx.send(Message::Error(format!("create task: {e}")));
                }
            }
        });
    }

    fn submit_assignees(&mut self) {
        let Some(task_id) = self.current_task_id() else {
            self.mode = Mode::Normal;
            self.assignee_editor_selections.clear();
            return;
        };
        let Some(task) = self.task_detail.as_ref() else {
            self.mode = Mode::Normal;
            self.assignee_editor_selections.clear();
            return;
        };
        let original: HashSet<i64> = task.assignees.iter().map(|a| a.id).collect();
        let new = self.assignee_editor_selections.clone();
        let to_add: Vec<i64> = new.difference(&original).copied().collect();
        let to_rem: Vec<i64> = original.difference(&new).copied().collect();
        self.mode = Mode::Normal;
        self.assignee_editor_selections.clear();
        if to_add.is_empty() && to_rem.is_empty() {
            return;
        }
        let body = serde_json::json!({
            "assignees": {
                "add": to_add,
                "rem": to_rem,
            }
        });
        self.spawn_update(task_id, body, "assignees");
    }
}

fn clamp(state: &mut ListState, len: usize) {
    if len == 0 {
        state.select(None);
    } else {
        let cur = state.selected().unwrap_or(0);
        state.select(Some(cur.min(len - 1)));
    }
}

fn api_list_to_entry(l: &ApiList) -> ListEntry {
    ListEntry {
        id: l.id.clone(),
        name: l.name.clone(),
        task_count: l.task_count,
    }
}

/// Count the number of tasks in a list that match the active assignee filter.
/// For unloaded lists, return the ClickUp-reported `task_count` when no
/// assignee filter is active, or `1` (optimistically not-empty) when an
/// assignee filter is active — so we don't prematurely hide a list whose
/// matches haven't loaded yet.
fn list_match_count(
    list_id: &str,
    fallback_count: Option<u32>,
    tasks_cache: &HashMap<String, Vec<Task>>,
    assignee: Option<&str>,
) -> usize {
    if let Some(tasks) = tasks_cache.get(list_id) {
        match assignee {
            None => tasks.len(),
            Some("(unassigned)") => tasks.iter().filter(|t| t.assignees.is_empty()).count(),
            Some(name) => tasks
                .iter()
                .filter(|t| t.assignees.iter().any(|a| a.username.eq_ignore_ascii_case(name)))
                .count(),
        }
    } else {
        let raw = fallback_count.unwrap_or(0) as usize;
        if assignee.is_some() {
            raw.min(1)
        } else {
            raw
        }
    }
}

/// Sum of `list_match_count` across every list in the folder. Folderless
/// `FolderEntry` (id=None) is interpreted as the current space's folderless
/// lists.
fn folder_match_count(
    fe: &FolderEntry,
    space_id: &str,
    space_contents: &HashMap<String, SpaceContents>,
    tasks_cache: &HashMap<String, Vec<Task>>,
    assignee: Option<&str>,
) -> usize {
    let Some(contents) = space_contents.get(space_id) else {
        return 0;
    };
    match &fe.id {
        Some(fid) => contents
            .folders
            .iter()
            .find(|f| f.id == *fid)
            .map(|f| {
                f.lists
                    .iter()
                    .map(|l| list_match_count(&l.id, l.task_count, tasks_cache, assignee))
                    .sum()
            })
            .unwrap_or(0),
        None => contents
            .folderless
            .iter()
            .map(|l| list_match_count(&l.id, l.task_count, tasks_cache, assignee))
            .sum(),
    }
}

/// Map a ClickUp priority name (case-insensitive) to its integer id 1-4.
fn priority_id_for_name(name: &str) -> Option<u8> {
    match name.to_ascii_lowercase().as_str() {
        "urgent" => Some(1),
        "high" => Some(2),
        "normal" => Some(3),
        "low" => Some(4),
        _ => None,
    }
}

/// True if the task has a due date strictly before `now_ms`.
fn is_overdue(t: &Task, now_ms: i64) -> bool {
    t.due_date
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<i64>().ok())
        .map(|due| due < now_ms)
        .unwrap_or(false)
}

/// Parse user date input to epoch millis. `Ok(None)` clears the date.
/// Accepts: empty/none/clear, today, tomorrow, yesterday, +Nd, YYYY-MM-DD,
/// MM/DD/YYYY, MM/DD/YY, MMM D YYYY, MMM D (current year).
fn parse_due_date_input(input: &str) -> Result<Option<i64>, String> {
    use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone, Utc};
    let raw = input.trim();
    let lower = raw.to_lowercase();

    if lower.is_empty() || lower == "none" || lower == "clear" || lower == "off" {
        return Ok(None);
    }

    let today = Local::now().date_naive();

    let from_keyword = match lower.as_str() {
        "today" => Some(today),
        "tomorrow" | "tmrw" => Some(today + Duration::days(1)),
        "yesterday" => Some(today - Duration::days(1)),
        s if s.starts_with('+') || s.starts_with('-') => {
            let sign: i64 = if s.starts_with('-') { -1 } else { 1 };
            let digits: String = s[1..].chars().take_while(|c| c.is_ascii_digit()).collect();
            digits
                .parse::<i64>()
                .ok()
                .map(|n| today + Duration::days(sign * n))
        }
        _ => None,
    };

    let date = from_keyword.or_else(|| {
        let with_year = [
            "%Y-%m-%d",
            "%m/%d/%Y",
            "%m/%d/%y",
            "%b %d %Y",
            "%b %d, %Y",
            "%B %d %Y",
            "%B %d, %Y",
        ];
        for fmt in with_year {
            if let Ok(d) = NaiveDate::parse_from_str(raw, fmt) {
                return Some(d);
            }
        }
        // year-less variants — assume current year
        let yr = today.year();
        let candidates: [(String, &str); 3] = [
            (format!("{raw} {yr}"), "%m/%d %Y"),
            (format!("{raw} {yr}"), "%b %d %Y"),
            (format!("{raw} {yr}"), "%B %d %Y"),
        ];
        for (s, fmt) in &candidates {
            if let Ok(d) = NaiveDate::parse_from_str(s, fmt) {
                return Some(d);
            }
        }
        None
    });

    let date = date.ok_or_else(|| format!("can't parse '{raw}'"))?;
    let dt = Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap());
    Ok(Some(dt.timestamp_millis()))
}

