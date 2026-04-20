use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        cursor::SetCursorStyle,
        event::{
            self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent,
            KeyModifiers, MouseButton, MouseEventKind,
        },
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Terminal,
};
use serde_json::json;
use std::env;
use std::fmt::Write as _;
use std::io;
use std::process::Command;
use std::time::Instant;

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub(crate) const ALL_SLASH_COMMANDS: &[(&str, &str)] = &[
    ("/exit", "Exit the TUI"),
    ("/quit", "Exit the TUI (alias for /exit)"),
    ("/help", "Show help information"),
    ("/status", "Show current session status"),
    ("/compact", "Compact the conversation"),
    ("/clear", "Clear conversation history"),
    ("/cost", "Show cost breakdown"),
    ("/stats", "Show usage statistics"),
    ("/session", "Manage sessions (list, switch, etc.)"),
    ("/mcp", "Inspect or bootstrap configured MCP servers"),
    ("/diagnostics", "Show LSP diagnostics for a file"),
    ("/symbols", "List document symbols for a file"),
    ("/references", "Find references at a file position"),
    ("/definition", "Go to definition at a file position"),
    ("/hover", "Show hover information at a file position"),
    ("/history", "Show command history"),
    ("/tokens", "Show token count for this conversation"),
    ("/cache", "Show prompt cache statistics"),
    ("/model", "Switch AI model"),
    ("/permissions", "Change permission mode"),
    ("/commit", "Create a git commit"),
    ("/pr", "Create a pull request"),
    ("/issue", "Create an issue"),
    ("/ultraplan", "Run ultra planning mode"),
    ("/teleport", "Navigate to location"),
    ("/bughunter", "Inspect codebase for likely bugs"),
    ("/debug-tool-call", "Debug tool execution"),
    ("/init", "Initialize project"),
    ("/theme", "Change visual theme"),
    ("/effort", "Set reasoning effort level"),
    ("/branch", "Switch git branch"),
];

const TUI_SAFE_SLASH_ROOTS: &[&str] = &[
    "help",
    "status",
    "compact",
    "clear",
    "cost",
    "stats",
    "tokens",
    "cache",
    "session",
    "mcp",
    "diagnostics",
    "symbols",
    "references",
    "definition",
    "hover",
    "model",
    "permissions",
    "bughunter",
    "commit",
    "pr",
    "issue",
    "ultraplan",
    "teleport",
    "debug-tool-call",
    "init",
    // Local TUI commands currently implemented in this module.
    "exit",
    "quit",
    "theme",
    "effort",
    "branch",
];

pub(crate) struct DraculaColors;

impl DraculaColors {
    pub(crate) const BACKGROUND: Color = Color::Rgb(40, 42, 54);
    pub(crate) const CURRENT_LINE: Color = Color::Rgb(68, 71, 90);
    pub(crate) const FOREGROUND: Color = Color::Rgb(248, 248, 242);
    pub(crate) const COMMENT: Color = Color::Rgb(98, 114, 164);
    pub(crate) const CYAN: Color = Color::Rgb(139, 233, 253);
    pub(crate) const GREEN: Color = Color::Rgb(80, 250, 123);
    pub(crate) const ORANGE: Color = Color::Rgb(255, 184, 108);
    pub(crate) const PINK: Color = Color::Rgb(255, 121, 198);
    pub(crate) const PURPLE: Color = Color::Rgb(189, 147, 249);
    pub(crate) const RED: Color = Color::Rgb(255, 85, 85);
    pub(crate) const YELLOW: Color = Color::Rgb(241, 250, 140);
    pub(crate) const BORDER: Color = Self::COMMENT;
    pub(crate) const TITLE: Color = Self::PINK;
    pub(crate) const USER_MSG: Color = Self::CYAN;
    pub(crate) const ASSISTANT_MSG: Color = Self::GREEN;
    pub(crate) const SYSTEM_MSG: Color = Self::YELLOW;
    pub(crate) const INPUT_BG: Color = Color::Rgb(50, 52, 66);
    pub(crate) const SELECTION_BG: Color = Self::PURPLE;
    pub(crate) const STATUS_BAR_BG: Color = Self::CURRENT_LINE;
    pub(crate) const ACCENT: Color = Self::CYAN;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Role {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatLine {
    pub(crate) role: Role,
    pub(crate) text: String,
    pub(crate) is_collapsed: bool,
    pub(crate) tool_name: Option<String>,
}

pub(crate) enum TuiOutcome {
    Submit(String),
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TuiMode {
    Normal,
    Pager {
        content: String,
        scroll: usize,
    },
    Help,
    SessionList {
        sessions: Vec<(String, String)>,
        selected: usize,
    },
    ModelList {
        models: Vec<(String, String)>,
        selected: usize,
    },
    PermissionMode {
        modes: Vec<(String, String)>,
        selected: usize,
    },
    ThemeList {
        themes: Vec<(String, String)>,
        selected: usize,
    },
    ToolList {
        tools: Vec<(String, String)>,
        selected: usize,
    },
    PluginList {
        plugins: Vec<(String, String)>,
        selected: usize,
    },
    AgentList {
        agents: Vec<(String, String)>,
        selected: usize,
    },
    SkillList {
        skills: Vec<(String, String)>,
        selected: usize,
    },
    BranchList {
        branches: Vec<(String, bool)>,
        selected: usize,
    },
    EffortList {
        levels: Vec<(String, String)>,
        selected: usize,
    },
    GenericList {
        title: String,
        items: Vec<(String, String)>,
        selected: usize,
    },
}

pub(crate) struct SessionInfo {
    pub id: String,
    pub messages: usize,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub cost: f64,
    pub created_at: String,
}

pub(crate) struct GitStatus {
    pub branch: String,
    pub status: String,
    pub files_changed: usize,
    pub files_staged: usize,
    pub ahead: usize,
    pub modified_files: Vec<ModifiedFile>,
}

#[derive(Debug, Clone)]
pub(crate) struct ModifiedFile {
    pub path: String,
    pub added: usize,
    pub removed: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolExecution {
    pub(crate) name: String,
    pub(crate) status: ToolStatus,
    pub(crate) duration_ms: u64,
    pub(crate) result_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolStatus {
    Running,
    Success,
    Failed,
}

pub(crate) struct ActiveTools {
    pub(crate) tools: Vec<String>,
    pub(crate) executing: Vec<ToolExecution>,
    pub(crate) mcp_servers: Vec<McpStatus>,
}

#[derive(Debug, Clone)]
pub(crate) struct McpStatus {
    pub name: String,
    pub status: String,
}

pub(crate) struct Permissions {
    pub mode: String,
    pub risk_level: String,
    pub last_approval: String,
}

pub(crate) struct QuickActions {
    pub actions: Vec<(String, String)>,
}

pub(crate) struct ResourceUsage {
    pub cpu_percent: u8,
    pub mem_mb: u32,
    pub disk_used_gb: u32,
    pub disk_total_gb: u32,
    pub net_up_mb: f32,
    pub net_down_mb: f32,
}

pub(crate) struct TuiApp {
    input: String,
    conversation: Vec<ChatLine>,
    slash_completions: Vec<String>,
    slash_matches: Vec<String>,
    slash_selected: usize,
    scroll_offset: usize,
    streaming_buffer: String,
    is_streaming: bool,
    is_thinking: bool,
    thinking_start_time: Option<Instant>,
    current_tool: Option<String>,
    turn_start_time: Option<Instant>,
    mode: TuiMode,
    current_model: String,
    session_id: String,
    message_count: usize,
    total_tokens_in: u32,
    total_tokens_out: u32,
    total_cost: f64,
    total_tokens_limit: u32,
    session_info: SessionInfo,
    git_status: GitStatus,
    active_tools: ActiveTools,
    permissions: Permissions,
    quick_actions: QuickActions,
    resource_usage: ResourceUsage,
    lsps: Vec<String>,
    mcp_dropdown_open: bool,
    lsp_dropdown_open: bool,
}

impl TuiApp {
    pub(crate) fn new() -> io::Result<Self> {
        Ok(Self {
            input: String::new(),
            conversation: Vec::new(),
            slash_completions: Vec::new(),
            slash_matches: Vec::new(),
            slash_selected: 0,
            scroll_offset: 0,
            streaming_buffer: String::new(),
            is_streaming: false,
            is_thinking: false,
            thinking_start_time: None,
            current_tool: None,
            turn_start_time: None,
            mode: TuiMode::Normal,
            current_model: "unknown".to_string(),
            session_id: String::new(),
            message_count: 0,
            total_tokens_in: 0,
            total_tokens_out: 0,
            total_cost: 0.0,
            total_tokens_limit: 64_000,
            session_info: SessionInfo {
                id: String::new(),
                messages: 0,
                tokens_in: 0,
                tokens_out: 0,
                cost: 0.0,
                created_at: String::new(),
            },
            git_status: GitStatus {
                branch: String::new(),
                status: String::new(),
                files_changed: 0,
                files_staged: 0,
                ahead: 0,
                modified_files: Vec::new(),
            },
            active_tools: ActiveTools {
                tools: Vec::new(),
                executing: Vec::new(),
                mcp_servers: Vec::new(),
            },
            permissions: Permissions {
                mode: String::new(),
                risk_level: String::new(),
                last_approval: String::new(),
            },
            quick_actions: QuickActions {
                actions: Vec::new(),
            },
            resource_usage: ResourceUsage {
                cpu_percent: 0,
                mem_mb: 0,
                disk_used_gb: 0,
                disk_total_gb: 0,
                net_up_mb: 0.0,
                net_down_mb: 0.0,
            },
            lsps: Vec::new(),
            mcp_dropdown_open: true,
            lsp_dropdown_open: true,
        })
    }

    pub(crate) fn set_completions(&mut self, completions: Vec<String>) {
        self.slash_completions = completions;
        self.recompute_slash_matches();
    }

    pub(crate) fn push_line(&mut self, role: Role, text: impl Into<String>) {
        self.conversation.push(ChatLine {
            role,
            text: text.into(),
            is_collapsed: false,
            tool_name: None,
        });
        self.scroll_offset = 0;
    }

    pub(crate) fn render_diff_to_conversation(&mut self, diff_text: &str) {
        let colored_diff = self.format_colored_diff(diff_text);
        self.push_line(Role::System, colored_diff);
    }

    fn format_colored_diff(&self, diff_text: &str) -> String {
        let mut result = String::new();
        for line in diff_text.lines() {
            if line.starts_with('+') && !line.starts_with("+++") {
                let _ = writeln!(result, "🟢 {line}");
            } else if line.starts_with('-') && !line.starts_with("---") {
                let _ = writeln!(result, "🔴 {line}");
            } else if line.starts_with("@@") {
                let _ = writeln!(result, "📍 {line}");
            } else {
                let _ = writeln!(result, "  {line}");
            }
        }
        result
    }

    pub(crate) fn push_tool_result(
        &mut self,
        tool_name: String,
        text: impl Into<String>,
        collapsed: bool,
    ) {
        self.conversation.push(ChatLine {
            role: Role::System,
            text: text.into(),
            is_collapsed: collapsed,
            tool_name: Some(tool_name),
        });
        self.scroll_offset = 0;
    }

    pub(crate) fn toggle_collapsed(&mut self, index: usize) {
        if let Some(line) = self.conversation.get_mut(index) {
            line.is_collapsed = !line.is_collapsed;
        }
    }

    pub(crate) fn start_thinking(&mut self) {
        self.is_thinking = true;
        self.thinking_start_time = Some(Instant::now());
    }

    pub(crate) fn stop_thinking(&mut self) {
        self.is_thinking = false;
    }

    pub(crate) fn start_streaming(&mut self) {
        self.is_streaming = true;
        self.streaming_buffer.clear();
    }

    pub(crate) fn stream_text(&mut self, text: &str) {
        if self.is_streaming {
            self.streaming_buffer.push_str(text);
        }
    }

    pub(crate) fn stop_streaming(&mut self) {
        if self.is_streaming {
            self.is_streaming = false;
            if !self.streaming_buffer.is_empty() {
                let text = self.streaming_buffer.clone();
                self.push_line(Role::Assistant, text);
                self.streaming_buffer.clear();
            }
        }
    }

    pub(crate) fn update_session_info(
        &mut self,
        messages: usize,
        tokens_in: u32,
        tokens_out: u32,
        cost: f64,
    ) {
        self.message_count = messages;
        self.session_info.messages = messages;
        self.session_info.tokens_in = tokens_in;
        self.session_info.tokens_out = tokens_out;
        self.session_info.cost = cost;
    }

    pub(crate) fn start_turn(&mut self) {
        self.turn_start_time = Some(Instant::now());
    }

    pub(crate) fn get_turn_duration_secs(&self) -> u64 {
        self.turn_start_time.map_or(0, |t| t.elapsed().as_secs())
    }

    pub(crate) fn update_usage(&mut self, input_tokens: u32, output_tokens: u32, cost: f64) {
        self.total_tokens_in = input_tokens;
        self.total_tokens_out = output_tokens;
        self.total_cost = cost;
    }

    pub(crate) fn update_git_status(
        &mut self,
        branch: String,
        status: String,
        changed: usize,
        staged: usize,
        ahead: usize,
    ) {
        self.git_status.branch = branch;
        self.git_status.status = status;
        self.git_status.files_changed = changed;
        self.git_status.files_staged = staged;
        self.git_status.ahead = ahead;
    }

    pub(crate) fn start_tool_execution(&mut self, tool_name: String) {
        self.active_tools.executing.push(ToolExecution {
            name: tool_name,
            status: ToolStatus::Running,
            duration_ms: 0,
            result_summary: None,
        });
    }

    pub(crate) fn complete_tool_execution(
        &mut self,
        tool_name: &str,
        success: bool,
        result: Option<String>,
    ) {
        if let Some(tool) = self
            .active_tools
            .executing
            .iter_mut()
            .rev()
            .find(|t| t.name == tool_name)
        {
            tool.status = if success {
                ToolStatus::Success
            } else {
                ToolStatus::Failed
            };
            tool.result_summary = result;
        }
    }

    pub(crate) fn clear_completed_tools(&mut self) {
        self.active_tools
            .executing
            .retain(|t| t.status == ToolStatus::Running);
    }

    pub(crate) fn enter_pager_mode(&mut self, content: String) {
        self.mode = TuiMode::Pager { content, scroll: 0 };
    }

    pub(crate) fn enter_help_mode(&mut self) {
        self.mode = TuiMode::Help;
    }

    pub(crate) fn enter_session_list(&mut self, sessions: Vec<(String, String)>) {
        self.mode = TuiMode::SessionList {
            sessions,
            selected: 0,
        };
    }

    pub(crate) fn enter_model_list(&mut self, models: Vec<(String, String)>) {
        self.mode = TuiMode::ModelList {
            models,
            selected: 0,
        };
    }

    pub(crate) fn enter_permission_mode(&mut self, modes: Vec<(String, String)>) {
        self.mode = TuiMode::PermissionMode { modes, selected: 0 };
    }

    pub(crate) fn enter_theme_list(&mut self, themes: Vec<(String, String)>) {
        self.mode = TuiMode::ThemeList {
            themes,
            selected: 0,
        };
    }

    pub(crate) fn enter_tool_list(&mut self, tools: Vec<(String, String)>) {
        self.mode = TuiMode::ToolList { tools, selected: 0 };
    }

    pub(crate) fn enter_plugin_list(&mut self, plugins: Vec<(String, String)>) {
        self.mode = TuiMode::PluginList {
            plugins,
            selected: 0,
        };
    }

    pub(crate) fn enter_agent_list(&mut self, agents: Vec<(String, String)>) {
        self.mode = TuiMode::AgentList {
            agents,
            selected: 0,
        };
    }

    pub(crate) fn enter_skill_list(&mut self, skills: Vec<(String, String)>) {
        self.mode = TuiMode::SkillList {
            skills,
            selected: 0,
        };
    }

    pub(crate) fn enter_branch_list(&mut self, branches: Vec<(String, bool)>) {
        self.mode = TuiMode::BranchList {
            branches,
            selected: 0,
        };
    }

    pub(crate) fn enter_effort_list(&mut self, levels: Vec<(String, String)>) {
        self.mode = TuiMode::EffortList {
            levels,
            selected: 0,
        };
    }

    pub(crate) fn enter_generic_list(&mut self, title: String, items: Vec<(String, String)>) {
        self.mode = TuiMode::GenericList {
            title,
            items,
            selected: 0,
        };
    }

    pub(crate) fn exit_mode(&mut self) {
        self.mode = TuiMode::Normal;
    }

    pub(crate) fn update_model_display(&mut self, model: &str) {
        self.current_model = model.to_string();
    }

    pub(crate) fn update_from_cli(&mut self, model: &str, session_id: &str, message_count: usize) {
        self.current_model = model.to_string();
        self.session_id = session_id.to_string();
        self.message_count = message_count;
        self.session_info.id = session_id.to_string();
        self.session_info.messages = message_count;
    }

    pub(crate) fn update_session_created_at(&mut self, created_at: String) {
        self.session_info.created_at = created_at;
    }

    pub(crate) fn update_permissions(&mut self, mode: &str, risk_level: &str, last_approval: &str) {
        self.permissions.mode = mode.to_string();
        self.permissions.risk_level = risk_level.to_string();
        self.permissions.last_approval = last_approval.to_string();
    }

    pub(crate) fn set_token_limit(&mut self, limit: u32) {
        self.total_tokens_limit = limit;
    }

    pub(crate) fn set_mcp_servers(&mut self, servers: Vec<String>) {
        // Backward-compatible helper used by older call sites.
        self.active_tools.mcp_servers = servers
            .into_iter()
            .map(|name| McpStatus {
                name,
                status: "connected".to_string(),
            })
            .collect();
    }

    pub(crate) fn set_mcp_statuses(&mut self, statuses: Vec<McpStatus>) {
        self.active_tools.mcp_servers = statuses;
    }

    pub(crate) fn set_lsps(&mut self, lsps: Vec<String>) {
        self.lsps = lsps;
    }

    pub(crate) fn detect_git_status(&mut self) {
        if let Ok(output) = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
        {
            if output.status.success() {
                self.git_status.branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            }
        }
        if let Ok(output) = Command::new("git").args(["status", "--porcelain"]).output() {
            if output.status.success() {
                let status_output = String::from_utf8_lossy(&output.stdout);
                let lines: Vec<&str> = status_output.lines().collect();
                let changed = lines.iter().filter(|line| !line.starts_with("??")).count();
                let staged = lines
                    .iter()
                    .filter(|line| {
                        line.starts_with("M ") || line.starts_with("A ") || line.starts_with("D ")
                    })
                    .count();
                self.git_status.files_changed = changed;
                self.git_status.files_staged = staged;
                self.git_status.status = if changed == 0 {
                    "✅ clean".to_string()
                } else {
                    format!("📝 {changed} files")
                };
                self.git_status.modified_files.clear();
                for line in lines {
                    if line.starts_with("## ") || line.trim().is_empty() || line.starts_with("??") {
                        continue;
                    }
                    let path = &line[3..];
                    if let Ok(diff) = Command::new("git")
                        .args(["diff", "--numstat", "HEAD", "--", path])
                        .output()
                    {
                        if diff.status.success() {
                            let stats = String::from_utf8_lossy(&diff.stdout);
                            if let Some(s) = stats.lines().next() {
                                let p: Vec<&str> = s.split_whitespace().collect();
                                if p.len() >= 2 {
                                    self.git_status.modified_files.push(ModifiedFile {
                                        path: path.to_string(),
                                        added: p[0].parse().unwrap_or(0),
                                        removed: p[1].parse().unwrap_or(0),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn run_interactive_with_cli(
        &mut self,
        cli: &mut crate::LiveCli,
        model: &str,
    ) -> io::Result<TuiOutcome> {
        self.update_from_cli(model, &cli.session.id, cli.prompt_history.len());
        self.detect_git_status();
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            SetCursorStyle::SteadyBar
        )?;
        let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        terminal.clear()?;
        loop {
            terminal.draw(|f| self.ui(f))?;
            if event::poll(std::time::Duration::from_millis(100))? {
                match event::read()? {
                    Event::Resize(_, _) => {
                        terminal.clear()?;
                    }
                    Event::Key(key) => {
                        if key.modifiers.contains(KeyModifiers::CONTROL)
                            && matches!(key.code, KeyCode::Char('c'))
                        {
                            cli.persist_session().ok();
                            disable_raw_mode()?;
                            execute!(
                                terminal.backend_mut(),
                                SetCursorStyle::DefaultUserShape,
                                DisableMouseCapture,
                                LeaveAlternateScreen
                            )?;
                            return Ok(TuiOutcome::Exit);
                        }
                        match key.code {
                            KeyCode::Enter => {
                                if matches!(self.mode, TuiMode::Normal) {
                                    let input = if self.has_slash_modal() {
                                        self.slash_matches.get(self.slash_selected).cloned()
                                    } else {
                                        let typed = self.input.clone();
                                        if typed.trim().is_empty() {
                                            None
                                        } else {
                                            Some(typed)
                                        }
                                    };

                                    if let Some(input) = input {
                                        self.input.clear();
                                        self.slash_matches.clear();
                                        if input == "/exit" || input == "/quit" {
                                            cli.persist_session().ok();
                                            disable_raw_mode()?;
                                            execute!(
                                                terminal.backend_mut(),
                                                SetCursorStyle::DefaultUserShape,
                                                DisableMouseCapture,
                                                LeaveAlternateScreen
                                            )?;
                                            return Ok(TuiOutcome::Exit);
                                        }
                                        if input.starts_with('/') {
                                            self.handle_slash_command_in_tui(&input, cli, model);
                                        } else {
                                            self.process_message_in_tui(&input, cli, model);
                                        }
                                    }
                                } else {
                                    self.handle_mode_enter(cli, model);
                                }
                            }
                            KeyCode::Esc => {
                                if !matches!(self.mode, TuiMode::Normal) {
                                    self.exit_mode();
                                } else if matches!(self.mode, TuiMode::Normal) {
                                    self.slash_matches.clear();
                                }
                            }
                            KeyCode::Char(c) => {
                                if matches!(self.mode, TuiMode::Pager { .. }) && c == 'q' {
                                    self.exit_mode();
                                } else if matches!(self.mode, TuiMode::Normal) {
                                    if c == 'M' {
                                        self.mcp_dropdown_open = !self.mcp_dropdown_open;
                                    } else if c == 'L' {
                                        self.lsp_dropdown_open = !self.lsp_dropdown_open;
                                    } else {
                                        self.input.push(c);
                                        self.recompute_slash_matches();
                                    }
                                }
                            }
                            KeyCode::Backspace => {
                                if matches!(self.mode, TuiMode::Normal) {
                                    self.input.pop();
                                    self.recompute_slash_matches();
                                }
                            }
                            KeyCode::Up => {
                                if !matches!(self.mode, TuiMode::Normal) {
                                    self.move_mode_selection_up();
                                } else if self.has_slash_modal() {
                                    self.slash_selected = self.slash_selected.saturating_sub(1);
                                } else {
                                    self.scroll_offset += 1;
                                }
                            }
                            KeyCode::Down => {
                                if !matches!(self.mode, TuiMode::Normal) {
                                    self.move_mode_selection_down();
                                } else if self.has_slash_modal() {
                                    if self.slash_selected + 1 < self.slash_matches.len() {
                                        self.slash_selected += 1;
                                    }
                                } else {
                                    self.scroll_offset = self.scroll_offset.saturating_sub(1);
                                }
                            }
                            KeyCode::Tab => {
                                if self.has_slash_modal() {
                                    self.apply_selected_slash_match();
                                }
                            }
                            _ => {}
                        }
                    }
                    Event::Mouse(mouse) => {
                        if matches!(self.mode, TuiMode::Normal)
                            && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                        {
                            let size = terminal.size()?;
                            let _ = self.toggle_active_tools_dropdown_from_click(
                                size.into(),
                                mouse.column,
                                mouse.row,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn toggle_active_tools_dropdown_from_click(
        &mut self,
        size: ratatui::prelude::Rect,
        mouse_x: u16,
        mouse_y: u16,
    ) -> bool {
        let layout = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Min(8),
                ratatui::layout::Constraint::Length(4),
                ratatui::layout::Constraint::Length(1),
            ])
            .split(size);
        let columns = ratatui::layout::Layout::default()
            .direction(if size.width < 110 {
                ratatui::layout::Direction::Vertical
            } else {
                ratatui::layout::Direction::Horizontal
            })
            .constraints(if size.width < 110 {
                vec![
                    ratatui::layout::Constraint::Min(12),
                    ratatui::layout::Constraint::Length(26),
                ]
            } else {
                vec![
                    ratatui::layout::Constraint::Percentage(70),
                    ratatui::layout::Constraint::Percentage(30),
                ]
            })
            .split(layout[0]);
        let sidebar = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Length(7),
                ratatui::layout::Constraint::Min(7),
            ])
            .split(columns[1]);
        let area = sidebar[1];

        let inner_x = area.x.saturating_add(1);
        let inner_y = area.y.saturating_add(1);
        let inner_w = area.width.saturating_sub(2);
        let inner_h = area.height.saturating_sub(2);

        if inner_w == 0 || inner_h == 0 {
            return false;
        }
        let inner_x_end = inner_x.saturating_add(inner_w);
        let inner_y_end = inner_y.saturating_add(inner_h);
        if mouse_x < inner_x || mouse_x >= inner_x_end || mouse_y < inner_y || mouse_y >= inner_y_end
        {
            return false;
        }

        // render_active_tools line indices (inside the block):
        // 0=status, 1=blank, 2=MCP header, (optional MCP items...), then LSP header.
        let mcp_header_y = inner_y.saturating_add(2);
        if mouse_y == mcp_header_y {
            self.mcp_dropdown_open = !self.mcp_dropdown_open;
            return true;
        }

        let mcp_items = if self.mcp_dropdown_open {
            if self.active_tools.mcp_servers.is_empty() {
                1
            } else {
                usize_to_u16_saturating(self.active_tools.mcp_servers.len())
            }
        } else {
            0
        };
        let lsp_header_y = mcp_header_y.saturating_add(1).saturating_add(mcp_items);
        if mouse_y == lsp_header_y {
            self.lsp_dropdown_open = !self.lsp_dropdown_open;
            return true;
        }

        false
    }

    fn handle_mode_enter(&mut self, cli: &mut crate::LiveCli, model: &str) {
        if let TuiMode::PermissionMode { modes, selected } = &self.mode {
            let selected_mode = modes.get(*selected).map(|(mode, _)| mode.clone());
            self.exit_mode();
            if let Some(mode) = selected_mode {
                match cli.set_permissions_for_tui(Some(mode)) {
                    Ok((should_persist, message)) => {
                        self.push_line(Role::System, message);
                        if should_persist {
                            if let Err(error) = cli.persist_session() {
                                self.push_line(
                                    Role::System,
                                    format!("Error persisting session: {error}"),
                                );
                            }
                        }
                        cli.refresh_tui_dashboard(self, model);
                    }
                    Err(error) => self.push_line(Role::System, format!("Error: {error}")),
                }
            }
            return;
        }

        let maybe_command = self.selected_mode_command();
        self.exit_mode();
        if let Some(command) = maybe_command {
            self.handle_slash_command_in_tui(&command, cli, model);
        }
    }

    fn selected_mode_command(&self) -> Option<String> {
        match &self.mode {
            TuiMode::SessionList { sessions, selected } => sessions
                .get(*selected)
                .map(|(session_id, _)| format!("/session switch {session_id}")),
            TuiMode::ModelList { models, selected } => models
                .get(*selected)
                .map(|(model, _)| format!("/model {model}")),
            TuiMode::PermissionMode { modes, selected } => modes
                .get(*selected)
                .map(|(mode, _)| format!("/permissions {mode}")),
            TuiMode::ThemeList { themes, selected } => themes
                .get(*selected)
                .map(|(theme, _)| format!("/theme {theme}")),
            TuiMode::SkillList { skills, selected } => skills
                .get(*selected)
                .map(|(skill, _)| format!("/skills {skill}")),
            TuiMode::BranchList { branches, selected } => branches
                .get(*selected)
                .map(|(branch, _)| format!("/branch {branch}")),
            TuiMode::EffortList { levels, selected } => levels
                .get(*selected)
                .map(|(level, _)| format!("/effort {level}")),
            _ => None,
        }
    }

    fn discover_branch_list(&self) -> Vec<(String, bool)> {
        let current = if self.git_status.branch.is_empty() {
            None
        } else {
            Some(self.git_status.branch.as_str())
        };

        let output = Command::new("git")
            .args(["branch", "--format=%(refname:short)"])
            .output();
        let Ok(output) = output else {
            return current
                .map(|branch| vec![(branch.to_string(), true)])
                .unwrap_or_default();
        };

        if !output.status.success() {
            return current
                .map(|branch| vec![(branch.to_string(), true)])
                .unwrap_or_default();
        }

        let mut branches = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let branch = line.trim();
            if branch.is_empty() {
                continue;
            }
            let is_current = current.is_some_and(|name| name == branch);
            branches.push((branch.to_string(), is_current));
        }

        if branches.is_empty() {
            current
                .map(|branch| vec![(branch.to_string(), true)])
                .unwrap_or_default()
        } else {
            branches
        }
    }

    fn move_mode_selection_up(&mut self) {
        match &mut self.mode {
            TuiMode::Pager { scroll, .. } => {
                *scroll = scroll.saturating_sub(1);
            }
            TuiMode::SessionList { selected, .. }
            | TuiMode::ModelList { selected, .. }
            | TuiMode::PermissionMode { selected, .. }
            | TuiMode::ThemeList { selected, .. }
            | TuiMode::ToolList { selected, .. }
            | TuiMode::PluginList { selected, .. }
            | TuiMode::AgentList { selected, .. }
            | TuiMode::SkillList { selected, .. }
            | TuiMode::BranchList { selected, .. }
            | TuiMode::EffortList { selected, .. }
            | TuiMode::GenericList { selected, .. } => {
                *selected = selected.saturating_sub(1);
            }
            TuiMode::Normal | TuiMode::Help => {}
        }
    }

    fn move_mode_selection_down(&mut self) {
        match &mut self.mode {
            TuiMode::Pager {
                content, scroll, ..
            } => {
                let max_scroll = content.lines().count().saturating_sub(1);
                if *scroll < max_scroll {
                    *scroll += 1;
                }
            }
            TuiMode::SessionList { sessions, selected } => {
                if *selected + 1 < sessions.len() {
                    *selected += 1;
                }
            }
            TuiMode::ModelList { models, selected } => {
                if *selected + 1 < models.len() {
                    *selected += 1;
                }
            }
            TuiMode::PermissionMode { modes, selected } => {
                if *selected + 1 < modes.len() {
                    *selected += 1;
                }
            }
            TuiMode::ThemeList { themes, selected } => {
                if *selected + 1 < themes.len() {
                    *selected += 1;
                }
            }
            TuiMode::ToolList { tools, selected } => {
                if *selected + 1 < tools.len() {
                    *selected += 1;
                }
            }
            TuiMode::PluginList { plugins, selected } => {
                if *selected + 1 < plugins.len() {
                    *selected += 1;
                }
            }
            TuiMode::AgentList { agents, selected } => {
                if *selected + 1 < agents.len() {
                    *selected += 1;
                }
            }
            TuiMode::SkillList { skills, selected } => {
                if *selected + 1 < skills.len() {
                    *selected += 1;
                }
            }
            TuiMode::BranchList { branches, selected } => {
                if *selected + 1 < branches.len() {
                    *selected += 1;
                }
            }
            TuiMode::EffortList { levels, selected } => {
                if *selected + 1 < levels.len() {
                    *selected += 1;
                }
            }
            TuiMode::GenericList {
                items, selected, ..
            } => {
                if *selected + 1 < items.len() {
                    *selected += 1;
                }
            }
            TuiMode::Normal | TuiMode::Help => {}
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle_slash_command_in_tui(&mut self, cmd: &str, cli: &mut crate::LiveCli, model: &str) {
        let trimmed = cmd.trim();
        if matches!(trimmed, "/session" | "/session list") {
            let session_list = match crate::list_managed_sessions_for_tui(&cli.session.id) {
                Ok(sessions) => sessions,
                Err(error) => {
                    self.push_line(
                        Role::System,
                        format!("Could not load managed sessions: {error}"),
                    );
                    Vec::new()
                }
            };
            if session_list.is_empty() {
                self.enter_generic_list(
                    "📁 Sessions".to_string(),
                    vec![("None".into(), "New session".into())],
                );
            } else {
                self.enter_session_list(session_list);
            }
            return;
        }

        if trimmed == "/model" {
            let mut models = match crate::fetch_ninerouter_models() {
                Ok(found) if !found.is_empty() => found
                    .into_iter()
                    .map(|(id, owner)| (format!("9router/{id}"), owner))
                    .collect(),
                Ok(_) => vec![
                    ("claude-sonnet-4".into(), "Default".into()),
                    ("claude-opus-4".into(), "High".into()),
                ],
                Err(error) => {
                    self.push_line(
                        Role::System,
                        format!("Could not load 9router models ({error}); using built-ins."),
                    );
                    vec![
                        ("claude-sonnet-4".into(), "Default".into()),
                        ("claude-opus-4".into(), "High".into()),
                    ]
                }
            };

            if !models.iter().any(|(name, _)| name == model) {
                models.insert(0, (model.to_string(), "Current".into()));
            }

            self.enter_model_list(models);
            return;
        }

        if trimmed == "/permissions" {
            self.enter_permission_mode(vec![
                (
                    "read-only".into(),
                    "Require explicit approval for edits".into(),
                ),
                (
                    "workspace-write".into(),
                    "Allow writes inside workspace".into(),
                ),
                (
                    "danger-full-access".into(),
                    "Full access without permission prompts".into(),
                ),
            ]);
            return;
        }

        if let Some(mode) = trimmed
            .strip_prefix("/permissions ")
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            match cli.set_permissions_for_tui(Some(mode.to_string())) {
                Ok((should_persist, message)) => {
                    self.push_line(Role::System, message);
                    if should_persist {
                        if let Err(error) = cli.persist_session() {
                            self.push_line(
                                Role::System,
                                format!("Error persisting session: {error}"),
                            );
                        }
                    }
                    cli.refresh_tui_dashboard(self, model);
                }
                Err(error) => self.push_line(Role::System, format!("Error: {error}")),
            }
            return;
        }

        if trimmed == "/theme" {
            self.enter_theme_list(vec![
                ("dracula".into(), "Dracula (default)".into()),
                ("dark".into(), "Dark".into()),
                ("light".into(), "Light".into()),
            ]);
            return;
        }

        if trimmed == "/effort" {
            self.enter_effort_list(vec![
                ("low".into(), "Fast, lower token budget".into()),
                ("medium".into(), "Balanced".into()),
                ("high".into(), "Deepest reasoning".into()),
            ]);
            return;
        }

        if trimmed == "/branch" {
            let branches = self.discover_branch_list();
            if branches.is_empty() {
                self.push_line(
                    Role::System,
                    "No git branches detected for the current workspace.",
                );
            } else {
                self.enter_branch_list(branches);
            }
            return;
        }

        if trimmed == "/clear" {
            self.conversation.clear();
            return;
        }

        if trimmed == "/help" {
            self.enter_help_mode();
            return;
        }

        if trimmed == "/status" {
            match cli.render_status_report() {
                Ok(report) => self.enter_pager_mode(report),
                Err(error) => self.push_line(Role::System, format!("Error: {error}")),
            }
            return;
        }

        if trimmed == "/mcp" || trimmed.starts_with("/mcp ") {
            self.handle_mcp_slash_in_tui(trimmed, cli, model);
            return;
        }

        if Self::is_lsp_shortcut_slash(trimmed) {
            self.handle_lsp_shortcut_slash_in_tui(trimmed, cli, model);
            return;
        }

        if matches!(trimmed, "/cost" | "/stats" | "/tokens" | "/cache") {
            self.enter_pager_mode(cli.render_cost_report());
            return;
        }

        if let Some(root) = Self::slash_command_root(trimmed) {
            if !Self::is_tui_safe_slash_root(root) {
                self.push_line(
                    Role::System,
                    format!("/{root} is not available in TUI mode yet."),
                );
                return;
            }
        }

        if Self::is_blocked_stub_slash_command(trimmed) {
            let root = Self::slash_command_root(trimmed).unwrap_or("unknown");
            self.push_line(
                Role::System,
                format!("/{root} is not yet implemented in this build."),
            );
            return;
        }

        match crate::SlashCommand::parse(trimmed) {
            Ok(Some(command)) => match command {
                crate::SlashCommand::Session { action, target }
                    if action.as_deref() == Some("switch") =>
                {
                    match cli.handle_session_command_result(action.as_deref(), target.as_deref()) {
                        Ok((should_persist, message)) => {
                            if let Some(message) = message {
                                self.push_line(Role::System, message);
                            }
                            if should_persist {
                                if let Err(error) = cli.persist_session() {
                                    self.push_line(
                                        Role::System,
                                        format!("Error persisting session: {error}"),
                                    );
                                }
                            }
                            cli.refresh_tui_dashboard(self, model);
                        }
                        Err(error) => {
                            self.push_line(Role::System, format!("Error: {error}"));
                        }
                    }
                }
                other => match cli.handle_repl_command(other) {
                    Ok(should_persist) => {
                        if should_persist {
                            if let Err(error) = cli.persist_session() {
                                self.push_line(
                                    Role::System,
                                    format!("Error persisting session: {error}"),
                                );
                            }
                        }
                        cli.refresh_tui_dashboard(self, model);
                    }
                    Err(error) => {
                        self.push_line(Role::System, format!("Error: {error}"));
                    }
                },
            },
            Ok(None) => {}
            Err(error) => {
                self.push_line(Role::System, error.to_string());
            }
        }
    }

    fn process_message_in_tui(&mut self, message: &str, cli: &mut crate::LiveCli, model: &str) {
        self.push_line(Role::User, message.to_string());
        self.start_thinking();
        cli.record_prompt_history(message);
        match cli.run_turn(message) {
            Ok(()) => {
                self.stop_thinking();
                cli.refresh_tui_dashboard(self, model);
            }
            Err(e) => {
                self.stop_thinking();
                self.push_line(Role::System, format!("Error: {e}"));
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiOutcome> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            return Some(TuiOutcome::Exit);
        }
        match key.code {
            KeyCode::Char(c) => {
                self.input.push(c);
                self.recompute_slash_matches();
                None
            }
            KeyCode::Backspace => {
                self.input.pop();
                self.recompute_slash_matches();
                None
            }
            KeyCode::Esc => {
                self.slash_matches.clear();
                None
            }
            _ => None,
        }
    }

    fn has_slash_modal(&self) -> bool {
        self.input.starts_with('/')
            && !self.slash_matches.is_empty()
            && !self
                .slash_matches
                .iter()
                .any(|candidate| candidate == &self.input)
    }

    fn slash_command_root(input: &str) -> Option<&str> {
        input
            .trim_start()
            .strip_prefix('/')?
            .split_whitespace()
            .next()
    }

    fn is_local_tui_slash_root(root: &str) -> bool {
        matches!(
            root,
            "exit"
                | "quit"
                | "help"
                | "clear"
                | "status"
                | "model"
                | "permissions"
                | "theme"
                | "effort"
                | "branch"
                | "session"
                | "mcp"
                | "diagnostics"
                | "symbols"
                | "references"
                | "definition"
                | "hover"
        )
    }

    fn is_lsp_shortcut_slash(input: &str) -> bool {
        matches!(
            Self::slash_command_root(input),
            Some("diagnostics" | "symbols" | "references" | "definition" | "hover")
        )
    }

    fn parse_optional_position(parts: &[&str]) -> Result<(Option<u32>, Option<u32>), String> {
        let line = match parts.first() {
            Some(value) => Some(
                value
                    .parse::<u32>()
                    .map_err(|_| format!("invalid line number: {value}"))?,
            ),
            None => None,
        };
        let character = match parts.get(1) {
            Some(value) => Some(
                value
                    .parse::<u32>()
                    .map_err(|_| format!("invalid character number: {value}"))?,
            ),
            None => None,
        };

        Ok((line, character))
    }

    fn handle_mcp_slash_in_tui(&mut self, input: &str, cli: &mut crate::LiveCli, model: &str) {
        let args = input.trim_start_matches("/mcp").trim();
        let args = (!args.is_empty()).then_some(args);
        let cwd = match env::current_dir() {
            Ok(cwd) => cwd,
            Err(error) => {
                self.push_line(
                    Role::System,
                    format!("Could not resolve current directory: {error}"),
                );
                return;
            }
        };

        match commands::handle_mcp_slash_command(args, &cwd) {
            Ok(report) => {
                self.enter_pager_mode(report);
                cli.refresh_tui_dashboard(self, model);
            }
            Err(error) => self.push_line(Role::System, format!("Error: {error}")),
        }
    }

    fn handle_lsp_shortcut_slash_in_tui(
        &mut self,
        input: &str,
        cli: &mut crate::LiveCli,
        model: &str,
    ) {
        let parts = input.split_whitespace().collect::<Vec<_>>();
        let Some(root) = parts.first().and_then(|value| value.strip_prefix('/')) else {
            self.push_line(Role::System, "Invalid slash command.".to_string());
            return;
        };

        let action = root;
        let mut payload = json!({ "action": action });

        match action {
            "diagnostics" => {
                if let Some(path) = parts.get(1) {
                    payload["path"] = json!(path);
                }
            }
            "symbols" => {
                let Some(path) = parts.get(1) else {
                    self.push_line(Role::System, "Usage: /symbols <path>".to_string());
                    return;
                };
                payload["path"] = json!(path);
            }
            "definition" | "references" | "hover" => {
                let Some(path) = parts.get(1) else {
                    self.push_line(
                        Role::System,
                        format!("Usage: /{action} <path> [line] [character]"),
                    );
                    return;
                };
                payload["path"] = json!(path);
                match Self::parse_optional_position(&parts[2..]) {
                    Ok((line, character)) => {
                        if let Some(line) = line {
                            payload["line"] = json!(line);
                        }
                        if let Some(character) = character {
                            payload["character"] = json!(character);
                        }
                    }
                    Err(error) => {
                        self.push_line(Role::System, format!("Error: {error}"));
                        return;
                    }
                }
            }
            _ => {
                self.push_line(Role::System, format!("Unsupported LSP command: /{action}"));
                return;
            }
        }

        match tools::execute_tool("LSP", &payload) {
            Ok(result) => {
                self.enter_pager_mode(result);
                cli.refresh_tui_dashboard(self, model);
            }
            Err(error) => self.push_line(Role::System, format!("Error: {error}")),
        }
    }

    fn is_tui_safe_slash_root(root: &str) -> bool {
        TUI_SAFE_SLASH_ROOTS.contains(&root)
    }

    fn is_blocked_stub_slash_command(input: &str) -> bool {
        let Some(root) = Self::slash_command_root(input) else {
            return false;
        };

        if Self::is_local_tui_slash_root(root) {
            return false;
        }

        crate::STUB_COMMANDS.contains(&root)
    }

    fn keep_supported_slash_candidate(candidate: &str) -> bool {
        let Some(root) = Self::slash_command_root(candidate) else {
            return false;
        };
        Self::is_tui_safe_slash_root(root) && !Self::is_blocked_stub_slash_command(candidate)
    }

    fn apply_selected_slash_match(&mut self) {
        if let Some(s) = self.slash_matches.get(self.slash_selected) {
            self.input = s.clone();
        }
        self.recompute_slash_matches();
    }
    fn recompute_slash_matches(&mut self) {
        if !self.input.starts_with('/') {
            self.slash_matches.clear();
            return;
        }

        if self.input == "/" {
            self.slash_matches = TUI_SAFE_SLASH_ROOTS
                .iter()
                .map(|root| format!("/{root}"))
                .collect();
        } else {
            self.slash_matches = commands::suggest_slash_commands(&self.input, 10)
                .into_iter()
                .filter(|candidate| Self::keep_supported_slash_candidate(candidate))
                .collect();

            if "/quit".starts_with(&self.input)
                && !self
                    .slash_matches
                    .iter()
                    .any(|candidate| candidate == "/quit")
            {
                self.slash_matches.push("/quit".to_string());
            }

            if "/exit".starts_with(&self.input)
                && !self
                    .slash_matches
                    .iter()
                    .any(|candidate| candidate == "/exit")
            {
                self.slash_matches.push("/exit".to_string());
            }
        }

        if self.slash_matches.len() > 10 {
            self.slash_matches.truncate(10);
        }

        if self.slash_selected >= self.slash_matches.len() {
            self.slash_selected = 0;
        }
    }

    fn token_usage_percent(&self) -> u32 {
        if self.total_tokens_limit == 0 {
            0
        } else {
            (self.total_tokens_in + self.total_tokens_out) * 100 / self.total_tokens_limit
        }
    }

    fn compact_session_id(&self) -> String {
        if self.session_info.id.is_empty() {
            "pending".to_string()
        } else {
            truncate_to_width(&self.session_info.id, 18)
        }
    }

    fn detected_project_name(&self) -> String {
        let cwd = std::env::current_dir().unwrap_or_default();
        let current_name = cwd
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "unknown".to_string());

        if current_name == "rust" {
            cwd.parent()
                .and_then(|parent| parent.file_name())
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or(current_name)
        } else {
            current_name
        }
    }

    fn current_dir_display(&self) -> String {
        let current_dir = std::env::current_dir().unwrap_or_default();
        if let Ok(home) = std::env::var("HOME") {
            current_dir.to_string_lossy().replace(&home, "~")
        } else {
            current_dir.to_string_lossy().to_string()
        }
    }

    fn active_agent_name(&self) -> &'static str {
        "CLAW Code"
    }

    fn sidebar_block<'a>(&self, title: &'a str, accent: Color) -> Block<'a> {
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(DraculaColors::CURRENT_LINE))
            .title(Line::from(vec![
                Span::styled(" ", Style::default().fg(DraculaColors::COMMENT)),
                Span::styled(
                    title,
                    Style::default().fg(accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ", Style::default().fg(DraculaColors::COMMENT)),
            ]))
            .style(Style::default().bg(DraculaColors::BACKGROUND))
    }

    fn render_session_info(&self, frame: &mut ratatui::Frame, area: ratatui::prelude::Rect) {
        let content = Paragraph::new(vec![
            Line::from(vec![
                Span::styled("🆔 SESSION ", Style::default().fg(DraculaColors::COMMENT)),
                Span::styled(
                    if self.session_info.id.is_empty() {
                        "pending"
                    } else {
                        &self.session_info.id
                    },
                    Style::default()
                        .fg(DraculaColors::FOREGROUND)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("💬 MESSAGES ", Style::default().fg(DraculaColors::COMMENT)),
                Span::styled(
                    self.session_info.messages.to_string(),
                    Style::default().fg(DraculaColors::CYAN),
                ),
            ]),
            Line::from(vec![
                Span::styled("🪙 TOKENS ", Style::default().fg(DraculaColors::COMMENT)),
                Span::styled(
                    format!(
                        "{} used · {}%",
                        self.total_tokens_in + self.total_tokens_out,
                        self.token_usage_percent()
                    ),
                    Style::default().fg(DraculaColors::FOREGROUND),
                ),
            ]),
            Line::from(vec![
                Span::styled("💵 COST ", Style::default().fg(DraculaColors::COMMENT)),
                Span::styled(
                    format!("${:.4}", self.session_info.cost),
                    Style::default().fg(DraculaColors::ORANGE),
                ),
            ]),
            Line::from(vec![
                Span::styled("🗓 STARTED ", Style::default().fg(DraculaColors::COMMENT)),
                Span::styled(
                    if self.session_info.created_at.is_empty() {
                        "Unavailable"
                    } else {
                        &self.session_info.created_at
                    },
                    Style::default().fg(DraculaColors::COMMENT),
                ),
            ]),
        ])
        .block(self.sidebar_block("🦀 Session Info", DraculaColors::CYAN))
        .wrap(Wrap { trim: true });
        frame.render_widget(content, area);
    }

    fn render_active_tools(&self, frame: &mut ratatui::Frame, area: ratatui::prelude::Rect) {
        let running_tool = self
            .active_tools
            .executing
            .iter()
            .rev()
            .find(|tool| tool.status == ToolStatus::Running)
            .map_or("idle", |tool| tool.name.as_str());

        let mcp_count = self.active_tools.mcp_servers.len();
        let lsp_count = self.lsps.len();
        let mut lines = vec![
            Line::from(vec![
                Span::styled("🔧 STATUS ", Style::default().fg(DraculaColors::COMMENT)),
                Span::styled(running_tool, Style::default().fg(DraculaColors::FOREGROUND)),
            ]),
            Line::from(""),
        ];

        let mcp_marker = if self.mcp_dropdown_open { "▾" } else { "▸" };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{mcp_marker} MCP"),
                Style::default()
                    .fg(DraculaColors::FOREGROUND)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {mcp_count}"),
                Style::default().fg(DraculaColors::COMMENT),
            ),
        ]));

        if self.mcp_dropdown_open {
            if self.active_tools.mcp_servers.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("  • none", Style::default().fg(DraculaColors::COMMENT)),
                ]));
            } else {
                for server in &self.active_tools.mcp_servers {
                    let status_color = if server.status.starts_with("error") {
                        DraculaColors::RED
                    } else if server.status == "pending" {
                        DraculaColors::ORANGE
                    } else {
                        DraculaColors::CYAN
                    };
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("  • {}", server.name),
                            Style::default().fg(DraculaColors::FOREGROUND),
                        ),
                        Span::raw(" "),
                        Span::styled(&server.status, Style::default().fg(status_color)),
                    ]));
                }
            }
        }

        let lsp_marker = if self.lsp_dropdown_open { "▾" } else { "▸" };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{lsp_marker} LSP"),
                Style::default()
                    .fg(DraculaColors::FOREGROUND)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {lsp_count}"),
                Style::default().fg(DraculaColors::COMMENT),
            ),
        ]));

        if self.lsp_dropdown_open {
            if self.lsps.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("  • none", Style::default().fg(DraculaColors::COMMENT)),
                ]));
            } else {
                for lsp in &self.lsps {
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("  • {lsp}"),
                            Style::default().fg(DraculaColors::FOREGROUND),
                        ),
                    ]));
                }
            }
        }

        if let Some(tool) = self
            .active_tools
            .executing
            .iter()
            .rev()
            .find(|tool| tool.status != ToolStatus::Running)
        {
            lines.push(Line::from(vec![
                Span::styled("↳ last ", Style::default().fg(DraculaColors::COMMENT)),
                Span::styled(&tool.name, Style::default().fg(DraculaColors::COMMENT)),
                Span::raw(" · "),
                Span::styled(
                    match tool.status {
                        ToolStatus::Success => "ok",
                        ToolStatus::Failed => "failed",
                        ToolStatus::Running => "running",
                    },
                    Style::default().fg(match tool.status {
                        ToolStatus::Success => DraculaColors::GREEN,
                        ToolStatus::Failed => DraculaColors::RED,
                        ToolStatus::Running => DraculaColors::ORANGE,
                    }),
                ),
            ]));
        }

        let content = Paragraph::new(lines)
            .block(self.sidebar_block("🔧 Active Tools", DraculaColors::ORANGE))
            .wrap(Wrap { trim: true });
        frame.render_widget(content, area);
    }

    fn render_status_bar(&self, frame: &mut ratatui::Frame, area: ratatui::prelude::Rect) {
        let branch = if self.git_status.branch.is_empty() {
            "no-branch"
        } else {
            &self.git_status.branch
        };
        let permission_mode = if self.permissions.mode.is_empty() {
            "unknown"
        } else {
            &self.permissions.mode
        };

        let chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([
                ratatui::layout::Constraint::Min(1),
                ratatui::layout::Constraint::Length(10),
            ])
            .split(area);

        let left = Paragraph::new(Line::from(vec![
            Span::styled(
                self.current_dir_display(),
                Style::default()
                    .bg(DraculaColors::STATUS_BAR_BG)
                    .fg(DraculaColors::COMMENT),
            ),
            Span::styled(
                "  •  ",
                Style::default()
                    .bg(DraculaColors::STATUS_BAR_BG)
                    .fg(DraculaColors::COMMENT),
            ),
            Span::styled(
                branch,
                Style::default()
                    .bg(DraculaColors::STATUS_BAR_BG)
                    .fg(DraculaColors::GREEN),
            ),
            Span::styled(
                "  •  ",
                Style::default()
                    .bg(DraculaColors::STATUS_BAR_BG)
                    .fg(DraculaColors::COMMENT),
            ),
            Span::styled(
                permission_mode,
                Style::default()
                    .bg(DraculaColors::STATUS_BAR_BG)
                    .fg(DraculaColors::ORANGE),
            ),
        ]))
        .style(Style::default().bg(DraculaColors::STATUS_BAR_BG));

        let right = Paragraph::new(Span::styled(
            "/command",
            Style::default()
                .bg(DraculaColors::STATUS_BAR_BG)
                .fg(DraculaColors::CYAN)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(DraculaColors::STATUS_BAR_BG))
        .alignment(ratatui::layout::Alignment::Right);

        frame.render_widget(left, chunks[0]);
        frame.render_widget(right, chunks[1]);
    }

    #[allow(clippy::too_many_lines)]
    fn ui(&self, frame: &mut ratatui::Frame) {
        let size = frame.area();
        frame.render_widget(Clear, size);
        frame
            .buffer_mut()
            .set_style(size, Style::default().bg(DraculaColors::BACKGROUND));
        let layout = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Min(8),
                ratatui::layout::Constraint::Length(4),
                ratatui::layout::Constraint::Length(1),
            ])
            .split(size);
        let columns = ratatui::layout::Layout::default()
            .direction(if size.width < 110 {
                ratatui::layout::Direction::Vertical
            } else {
                ratatui::layout::Direction::Horizontal
            })
            .constraints(if size.width < 110 {
                vec![
                    ratatui::layout::Constraint::Min(12),
                    ratatui::layout::Constraint::Length(26),
                ]
            } else {
                vec![
                    ratatui::layout::Constraint::Percentage(70),
                    ratatui::layout::Constraint::Percentage(30),
                ]
            })
            .split(layout[0]);
        let sidebar = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Length(7),
                ratatui::layout::Constraint::Min(7),
            ])
            .split(columns[1]);
        frame.render_widget(
            Paragraph::new(self.format_conversation())
                .style(
                    Style::default()
                        .bg(DraculaColors::BACKGROUND)
                        .fg(DraculaColors::FOREGROUND),
                )
                .block(self.sidebar_block("🦀 Conversation", DraculaColors::TITLE))
                .wrap(Wrap { trim: true })
                .scroll((usize_to_u16_saturating(self.scroll_offset), 0)),
            columns[0],
        );
        let compose_title = format!(
            "🤖 Agent {}  •  Model {}",
            self.active_agent_name(),
            self.current_model
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(&self.input),
            ]))
            .style(
                Style::default()
                    .bg(DraculaColors::BACKGROUND)
                    .fg(DraculaColors::FOREGROUND),
            )
            .block(self.sidebar_block(&compose_title, DraculaColors::ORANGE)),
            layout[1],
        );
        if matches!(self.mode, TuiMode::Normal) {
            let inner_width = layout[1].width.saturating_sub(2);
            let max_input_width = inner_width.saturating_sub(1);
            let input_width = usize_to_u16_saturating(self.input.chars().count());
            let clamped_width = input_width.min(max_input_width);
            let cursor_x = layout[1]
                .x
                .saturating_add(1)
                .saturating_add(clamped_width);
            let cursor_y = layout[1].y.saturating_add(1);
            frame.set_cursor_position((cursor_x, cursor_y));
        }
        self.render_session_info(frame, sidebar[0]);
        self.render_active_tools(frame, sidebar[1]);
        self.render_status_bar(frame, layout[2]);
        if self.has_slash_modal() && matches!(self.mode, TuiMode::Normal) {
            self.render_slash_modal(frame, size);
        }
        match &self.mode {
            TuiMode::Help => self.render_help_overlay(frame, size),
            TuiMode::Pager { content, scroll } => {
                self.render_pager_overlay(frame, size, content, *scroll);
            }
            TuiMode::SessionList { sessions, selected } => {
                self.render_generic_list_overlay(frame, size, "📁 Sessions", sessions, *selected);
            }
            TuiMode::ModelList { models, selected } => {
                self.render_generic_list_overlay(frame, size, "🤖 Models", models, *selected);
            }
            TuiMode::PermissionMode { modes, selected } => {
                self.render_generic_list_overlay(frame, size, "🔐 Permissions", modes, *selected);
            }
            TuiMode::ThemeList { themes, selected } => {
                self.render_generic_list_overlay(frame, size, "🎨 Themes", themes, *selected);
            }
            TuiMode::ToolList { tools, selected } => {
                self.render_generic_list_overlay(frame, size, "🔧 Tools", tools, *selected);
            }
            TuiMode::PluginList { plugins, selected } => {
                self.render_generic_list_overlay(frame, size, " Plugins", plugins, *selected);
            }
            TuiMode::AgentList { agents, selected } => {
                self.render_generic_list_overlay(frame, size, "🤖 Agents", agents, *selected);
            }
            TuiMode::SkillList { skills, selected } => {
                self.render_generic_list_overlay(frame, size, "⚡ Skills", skills, *selected);
            }
            TuiMode::BranchList { branches, selected } => {
                let items: Vec<(String, String)> = branches
                    .iter()
                    .map(|(name, is_current)| {
                        (
                            name.clone(),
                            if *is_current {
                                "(current)".to_string()
                            } else {
                                String::new()
                            },
                        )
                    })
                    .collect();
                self.render_generic_list_overlay(frame, size, "🌿 Branches", &items, *selected);
            }
            TuiMode::EffortList { levels, selected } => self.render_generic_list_overlay(
                frame,
                size,
                "🧠 Reasoning Effort",
                levels,
                *selected,
            ),
            TuiMode::GenericList {
                title,
                items,
                selected,
            } => self.render_generic_list_overlay(frame, size, title, items, *selected),
            TuiMode::Normal => {}
        }
    }

    #[allow(clippy::too_many_lines)]
    fn format_conversation(&self) -> Text<'_> {
        let mut lines = Vec::new();

        for line in &self.conversation {
            if let Some(ref tool_name) = line.tool_name {
                if line.is_collapsed {
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("⚙️ {tool_name} "),
                            Style::default()
                                .fg(DraculaColors::YELLOW)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("[{}] [+] Expand", line.text.lines().count()),
                            Style::default().fg(DraculaColors::COMMENT),
                        ),
                    ]));
                } else {
                    lines.push(Line::from(vec![Span::styled(
                        format!("⚙️ {tool_name} [-]"),
                        Style::default()
                            .fg(DraculaColors::YELLOW)
                            .add_modifier(Modifier::BOLD),
                    )]));
                    let display_lines: Vec<&str> = line.text.lines().take(50).collect();
                    if line.text.lines().count() > 50 {
                        for display in display_lines {
                            lines.push(Line::from(vec![Span::raw(display.to_string())]));
                        }
                        lines.push(Line::from(vec![Span::styled(
                            format!("... ({} more lines)", line.text.lines().count() - 50),
                            Style::default().fg(DraculaColors::COMMENT),
                        )]));
                    } else {
                        if line.text.is_empty() {
                            lines.push(Line::from(""));
                        } else {
                            for part in line.text.lines() {
                                lines.push(Line::from(vec![Span::raw(part.to_string())]));
                            }
                            if line.text.ends_with('\n') {
                                lines.push(Line::from(""));
                            }
                        }
                    }
                }
                lines.push(Line::from(""));
            } else {
                let header = match line.role {
                    Role::User => Line::from(vec![
                        Span::styled(
                            "Sisyphus (Ultraworker) ",
                            Style::default()
                                .fg(DraculaColors::CYAN)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("· ", Style::default().fg(DraculaColors::COMMENT)),
                        Span::styled(
                            format!("{} ", self.current_model),
                            Style::default().fg(DraculaColors::COMMENT),
                        ),
                        Span::styled("· ", Style::default().fg(DraculaColors::COMMENT)),
                        Span::styled("just now", Style::default().fg(DraculaColors::COMMENT)),
                    ]),
                    Role::Assistant => Line::from(vec![
                        Span::styled(
                            "Sisyphus (Ultraworker) ",
                            Style::default()
                                .fg(DraculaColors::PURPLE)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("· ", Style::default().fg(DraculaColors::COMMENT)),
                        Span::styled(
                            format!("{} ", self.current_model),
                            Style::default().fg(DraculaColors::COMMENT),
                        ),
                        Span::styled("· ", Style::default().fg(DraculaColors::COMMENT)),
                        Span::styled("just now", Style::default().fg(DraculaColors::COMMENT)),
                    ]),
                    Role::System => Line::from(vec![Span::styled(
                        "⚙️ System ",
                        Style::default()
                            .fg(DraculaColors::YELLOW)
                            .add_modifier(Modifier::BOLD),
                    )]),
                };
                lines.push(header);
                if line.text.is_empty() {
                    lines.push(Line::from(""));
                } else {
                    for part in line.text.lines() {
                        lines.push(Line::from(vec![Span::raw(part.to_string())]));
                    }
                    if line.text.ends_with('\n') {
                        lines.push(Line::from(""));
                    }
                }
                lines.push(Line::from(""));
            }
        }
        if self.is_streaming && !self.streaming_buffer.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    "Sisyphus (Ultraworker) ",
                    Style::default()
                        .fg(DraculaColors::PURPLE)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("· ", Style::default().fg(DraculaColors::COMMENT)),
                Span::styled(
                    format!("{} ", self.current_model),
                    Style::default().fg(DraculaColors::COMMENT),
                ),
                Span::styled("· ", Style::default().fg(DraculaColors::COMMENT)),
                Span::styled("streaming...", Style::default().fg(DraculaColors::COMMENT)),
            ]));
            lines.push(Line::from(vec![
                Span::raw(&self.streaming_buffer),
                Span::styled(" ▊", Style::default().fg(DraculaColors::GREEN)),
            ]));
            lines.push(Line::from(""));
        }
        if self.is_thinking {
            let elapsed = self
                .thinking_start_time
                .map_or(0, |t| t.elapsed().as_secs());
            let spinner = SPINNER_FRAMES[u64_to_usize_saturating(elapsed) % SPINNER_FRAMES.len()];
            lines.push(Line::from(vec![
                Span::styled(
                    "🧠 Thinking",
                    Style::default()
                        .fg(DraculaColors::ORANGE)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(spinner, Style::default().fg(DraculaColors::ORANGE)),
                Span::raw(format!(" {elapsed}s")),
            ]));
            lines.push(Line::from(""));
        }
        Text::from(lines)
    }

    fn render_slash_modal(&self, frame: &mut ratatui::Frame, area: ratatui::prelude::Rect) {
        if self.slash_matches.is_empty() {
            return;
        }
        let popup_area = ratatui::layout::Rect {
            x: area.width / 4,
            y: area.height / 4,
            width: area.width / 2,
            height: std::cmp::min(12, usize_to_u16_saturating(self.slash_matches.len()) + 3),
        };
        frame.render_widget(Clear, popup_area);
        let items: Vec<ListItem> = self
            .slash_matches
            .iter()
            .enumerate()
            .map(|(i, cmd)| {
                let desc = ALL_SLASH_COMMANDS
                    .iter()
                    .find(|(c, _)| c == cmd)
                    .map(|(_, d)| *d)
                    .or_else(|| {
                        commands::slash_command_specs()
                            .iter()
                            .find(|spec| format!("/{}", spec.name) == *cmd)
                            .map(|spec| spec.summary)
                    })
                    .unwrap_or("");
                let style = if i == self.slash_selected {
                    Style::default()
                        .bg(DraculaColors::PURPLE)
                        .fg(DraculaColors::BACKGROUND)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .bg(DraculaColors::CURRENT_LINE)
                        .fg(DraculaColors::FOREGROUND)
                };

                let desc_style = if i == self.slash_selected {
                    Style::default()
                        .bg(DraculaColors::PURPLE)
                        .fg(DraculaColors::BACKGROUND)
                } else {
                    Style::default()
                        .bg(DraculaColors::CURRENT_LINE)
                        .fg(DraculaColors::COMMENT)
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!("{cmd:<20}"), style),
                    Span::styled(format!(" - {desc}"), desc_style),
                ]))
            })
            .collect();
        frame.render_widget(
            List::new(items)
                .style(Style::default().bg(DraculaColors::CURRENT_LINE))
                .block(
                    Block::default()
                        .title(" 🔍 Slash Commands ")
                        .borders(Borders::ALL)
                        .border_style(
                            Style::default()
                                .fg(DraculaColors::PURPLE)
                                .add_modifier(Modifier::BOLD),
                        )
                        .style(Style::default().bg(DraculaColors::CURRENT_LINE)),
                ),
            popup_area,
        );
    }

    fn render_help_overlay(&self, frame: &mut ratatui::Frame, size: ratatui::prelude::Rect) {
        let help_text = vec![
            "⌨️ Keyboard Shortcuts",
            "─────────────────────────",
            "",
            "Enter     Submit input",
            "Tab       Complete slash command",
            "Up/Down   Navigate conversation",
            "Esc       Clear modal/cancel",
            "?         Show this help",
            "q         Exit pager mode",
            "e         Expand/collapse tool output",
            "Shift+M   Toggle MCP dropdown",
            "Shift+L   Toggle LSP dropdown",
            "Ctrl+C    Exit TUI",
            "",
            "Press Esc to close",
        ];
        let help_area = ratatui::layout::Rect {
            x: size.width / 4,
            y: size.height / 6,
            width: size.width / 2,
            height: 18,
        };
        frame.render_widget(Clear, help_area);
        let text = Text::from(
            help_text
                .into_iter()
                .map(|s| Line::from(Span::raw(s)))
                .collect::<Vec<_>>(),
        );
        frame.render_widget(
            Paragraph::new(text)
                .style(
                    Style::default()
                        .bg(DraculaColors::CURRENT_LINE)
                        .fg(DraculaColors::FOREGROUND),
                )
                .block(
                    Block::default()
                        .title(" Help ")
                        .borders(Borders::ALL)
                        .border_style(
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        )
                        .style(Style::default().bg(DraculaColors::CURRENT_LINE)),
                )
                .alignment(ratatui::layout::Alignment::Center),
            help_area,
        );
    }

    fn render_pager_overlay(
        &self,
        frame: &mut ratatui::Frame,
        size: ratatui::prelude::Rect,
        content: &str,
        scroll: usize,
    ) {
        frame.render_widget(Clear, size);
        let lines: Vec<Line> = content
            .lines()
            .skip(scroll)
            .take((size.height as usize).saturating_sub(4))
            .map(|line| Line::from(Span::raw(line)))
            .collect();
        frame.render_widget(
            Paragraph::new(lines)
                .style(
                    Style::default()
                        .bg(DraculaColors::BACKGROUND)
                        .fg(DraculaColors::FOREGROUND),
                )
                .block(
                    Block::default()
                        .title(" Pager (q to quit) ")
                        .borders(Borders::ALL)
                        .border_style(
                            Style::default()
                                .fg(DraculaColors::YELLOW)
                                .add_modifier(Modifier::BOLD),
                        )
                        .style(Style::default().bg(DraculaColors::BACKGROUND)),
                )
                .wrap(Wrap { trim: true }),
            size,
        );
    }

    fn render_generic_list_overlay(
        &self,
        frame: &mut ratatui::Frame,
        size: ratatui::prelude::Rect,
        title: &str,
        items: &[(String, String)],
        selected: usize,
    ) {
        let popup_area = ratatui::layout::Rect {
            x: size.width / 6,
            y: size.height / 6,
            width: size.width * 2 / 3,
            height: std::cmp::min(15, usize_to_u16_saturating(items.len()) + 3),
        };
        frame.render_widget(Clear, popup_area);
        let list_items: Vec<ListItem> = items
            .iter()
            .enumerate()
            .map(|(i, (id, desc))| {
                let style = if i == selected {
                    Style::default()
                        .bg(DraculaColors::PURPLE)
                        .fg(DraculaColors::BACKGROUND)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .bg(DraculaColors::CURRENT_LINE)
                        .fg(DraculaColors::FOREGROUND)
                };

                let desc_style = if i == selected {
                    Style::default()
                        .bg(DraculaColors::PURPLE)
                        .fg(DraculaColors::BACKGROUND)
                } else {
                    Style::default()
                        .bg(DraculaColors::CURRENT_LINE)
                        .fg(DraculaColors::COMMENT)
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {id:<30}"), style),
                    Span::styled(format!(" - {desc}"), desc_style),
                ]))
            })
            .collect();
        frame.render_widget(
            List::new(list_items)
                .style(Style::default().bg(DraculaColors::CURRENT_LINE))
                .block(
                    Block::default()
                        .title(format!(" {title} "))
                        .borders(Borders::ALL)
                        .border_style(
                            Style::default()
                                .fg(DraculaColors::PURPLE)
                                .add_modifier(Modifier::BOLD),
                        )
                        .style(Style::default().bg(DraculaColors::CURRENT_LINE)),
                ),
            popup_area,
        );
    }
}

fn usize_to_u16_saturating(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn u64_to_usize_saturating(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

fn truncate_to_width(value: &str, width: usize) -> String {
    value.chars().take(width).collect()
}
