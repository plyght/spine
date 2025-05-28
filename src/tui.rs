use crate::config::Config;
use crate::detect::{DetectedManager, ManagerStatus};
use crate::execute::execute_manager_workflow;
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;
use std::sync::{Arc, Mutex};
use tokio::task::JoinSet;

#[derive(Debug, Clone, PartialEq)]
enum AppState {
    ManagerList,
    DetailView(usize),
    LogsView(usize),
}

#[derive(Debug, Clone)]
struct LogsViewState {
    scroll_offset: u16,
}

pub async fn run_tui(
    managers: Vec<DetectedManager>,
    _config: Config,
    selective: bool,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Convert managers to shared Arc<Mutex<>> for real-time updates
    let shared_managers: Vec<Arc<Mutex<DetectedManager>>> = managers
        .into_iter()
        .map(|m| Arc::new(Mutex::new(m)))
        .collect();

    let mut selected = 0;
    let mut list_state = ListState::default();
    list_state.select(Some(0));
    let mut app_state = AppState::ManagerList;

    // Track scroll state for each manager's logs view
    let mut logs_scroll_states: Vec<LogsViewState> = (0..shared_managers.len())
        .map(|_| LogsViewState { scroll_offset: 0 })
        .collect();

    // Track which managers have started their workflows
    let mut started_workflows: Vec<bool> = vec![false; shared_managers.len()];

    // Start all manager workflows in parallel (only if not in selective mode)
    let mut join_set = JoinSet::new();
    if !selective {
        for (i, manager_ref) in shared_managers.iter().enumerate() {
            let manager_ref = manager_ref.clone();
            started_workflows[i] = true;
            join_set.spawn(async move {
                let _ = execute_manager_workflow(manager_ref).await;
                i
            });
        }
    }

    loop {
        terminal.draw(|f| {
            ui(
                f,
                &shared_managers,
                &mut list_state,
                &app_state,
                &logs_scroll_states,
                selective,
            )
        })?;

        // Check for completed tasks
        while let Some(result) = join_set.try_join_next() {
            match result {
                Ok(_index) => {
                    // Task completed - manager state was updated via shared reference
                }
                Err(join_error) => {
                    // Log join errors but continue - individual manager failures are handled in the workflow
                    eprintln!("Task join error: {}", join_error);
                    break;
                }
            }
        }

        // Check if all managers are done
        let all_done = if selective {
            // In selective mode, only check started workflows
            shared_managers
                .iter()
                .enumerate()
                .filter(|(i, _)| started_workflows[*i])
                .all(|(_, m)| {
                    let manager = m.lock().unwrap();
                    matches!(
                        manager.status,
                        ManagerStatus::Success(_) | ManagerStatus::Failed(_)
                    )
                })
        } else {
            // In non-selective mode, check all managers
            shared_managers.iter().all(|m| {
                let manager = m.lock().unwrap();
                matches!(
                    manager.status,
                    ManagerStatus::Success(_) | ManagerStatus::Failed(_)
                )
            })
        };

        // Handle input
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match (&app_state, key.code) {
                        // Global quit commands
                        (_, KeyCode::Char('q')) => break,
                        (AppState::DetailView(_) | AppState::LogsView(_), KeyCode::Esc) => {
                            app_state = AppState::ManagerList;
                        }
                        // Manager list navigation
                        (AppState::ManagerList, KeyCode::Down | KeyCode::Char('j')) => {
                            if selected < shared_managers.len() - 1 {
                                selected += 1;
                                list_state.select(Some(selected));
                            }
                        }
                        (AppState::ManagerList, KeyCode::Up | KeyCode::Char('k')) => {
                            if selected > 0 {
                                selected -= 1;
                                list_state.select(Some(selected));
                            }
                        }
                        (AppState::ManagerList, KeyCode::Enter) => {
                            app_state = AppState::DetailView(selected);
                        }
                        // Selective mode: start workflow for selected manager
                        (AppState::ManagerList, KeyCode::Char(' ')) if selective => {
                            if selected < shared_managers.len() && !started_workflows[selected] {
                                let manager_ref = shared_managers[selected].clone();
                                let index = selected;
                                started_workflows[selected] = true;
                                join_set.spawn(async move {
                                    let _ = execute_manager_workflow(manager_ref).await;
                                    index
                                });
                            }
                        }
                        // Detail view navigation
                        (AppState::DetailView(manager_index), KeyCode::Char('l')) => {
                            app_state = AppState::LogsView(*manager_index);
                        }
                        (
                            AppState::DetailView(_) | AppState::LogsView(_),
                            KeyCode::Char('h') | KeyCode::Left,
                        ) => {
                            app_state = AppState::ManagerList;
                        }
                        // Logs view scrolling
                        (AppState::LogsView(manager_index), KeyCode::Up | KeyCode::Char('k')) => {
                            if let Some(scroll_state) = logs_scroll_states.get_mut(*manager_index) {
                                scroll_state.scroll_offset =
                                    scroll_state.scroll_offset.saturating_sub(1);
                            }
                        }
                        (AppState::LogsView(manager_index), KeyCode::Down | KeyCode::Char('j')) => {
                            if let Some(scroll_state) = logs_scroll_states.get_mut(*manager_index) {
                                scroll_state.scroll_offset =
                                    scroll_state.scroll_offset.saturating_add(1);
                            }
                        }
                        (AppState::LogsView(manager_index), KeyCode::PageUp) => {
                            if let Some(scroll_state) = logs_scroll_states.get_mut(*manager_index) {
                                scroll_state.scroll_offset =
                                    scroll_state.scroll_offset.saturating_sub(10);
                            }
                        }
                        (AppState::LogsView(manager_index), KeyCode::PageDown) => {
                            if let Some(scroll_state) = logs_scroll_states.get_mut(*manager_index) {
                                scroll_state.scroll_offset =
                                    scroll_state.scroll_offset.saturating_add(10);
                            }
                        }
                        (AppState::LogsView(manager_index), KeyCode::Home) => {
                            if let Some(scroll_state) = logs_scroll_states.get_mut(*manager_index) {
                                scroll_state.scroll_offset = 0;
                            }
                        }
                        (AppState::LogsView(manager_index), KeyCode::End) => {
                            if let Some(scroll_state) = logs_scroll_states.get_mut(*manager_index) {
                                // Set to a high value - the render function will clamp it appropriately
                                scroll_state.scroll_offset = u16::MAX;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Auto-exit when all done and showing summary
        if all_done && app_state == AppState::ManagerList {
            // Show final state for a moment before exiting
            if event::poll(std::time::Duration::from_millis(2000))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Convert shared managers back to regular managers for summary
    let final_managers: Vec<DetectedManager> = shared_managers
        .iter()
        .map(|m| m.lock().unwrap().clone())
        .collect();

    print_summary(&final_managers);

    Ok(())
}

fn ui(
    f: &mut Frame,
    shared_managers: &[Arc<Mutex<DetectedManager>>],
    list_state: &mut ListState,
    app_state: &AppState,
    logs_scroll_states: &[LogsViewState],
    selective: bool,
) {
    match app_state {
        AppState::ManagerList => {
            render_manager_list(f, shared_managers, list_state, selective);
        }
        AppState::DetailView(manager_index) => {
            if let Some(manager_ref) = shared_managers.get(*manager_index) {
                let manager = manager_ref.lock().unwrap();
                render_detail_view(f, &manager);
            }
        }
        AppState::LogsView(manager_index) => {
            if let Some(manager_ref) = shared_managers.get(*manager_index) {
                if let Some(scroll_state) = logs_scroll_states.get(*manager_index) {
                    let manager = manager_ref.lock().unwrap();
                    render_logs_view(f, &manager, scroll_state);
                }
            }
        }
    }
}

fn render_manager_list(
    f: &mut Frame,
    shared_managers: &[Arc<Mutex<DetectedManager>>],
    list_state: &mut ListState,
    selective: bool,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
        .split(f.area());

    let items: Vec<ListItem> = shared_managers
        .iter()
        .map(|manager_ref| {
            let manager = manager_ref.lock().unwrap();

            let status_style = match manager.status {
                ManagerStatus::Success(_) => Style::default().fg(Color::Green),
                ManagerStatus::Failed(_) => Style::default().fg(Color::Red),
                _ => Style::default().fg(Color::Yellow),
            };

            let status_text = match &manager.status {
                ManagerStatus::Pending => "Pending".to_string(),
                ManagerStatus::Running(operation, _) => format!("{}...", operation),
                ManagerStatus::Success(_) => "✓ Complete".to_string(),
                ManagerStatus::Failed(_err) => "✗ Failed".to_string(),
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<20}", manager.name), Style::default()),
                Span::styled(status_text, status_style),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Package Managers - Spine"),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_stateful_widget(list, chunks[0], list_state);

    // Help text
    let help_text = if selective {
        Paragraph::new("Navigate: ↑↓/j k | Start: Space | Detail: Enter | Quit: q")
    } else {
        Paragraph::new("Navigate: ↑↓/j k | Detail: Enter | Quit: q")
    }
    .block(Block::default().borders(Borders::ALL).title("Help"))
    .style(Style::default().fg(Color::Cyan));

    f.render_widget(help_text, chunks[1]);
}

fn render_detail_view(f: &mut Frame, manager: &DetectedManager) {
    let area = f.area().inner(Margin {
        horizontal: 2,
        vertical: 1,
    });

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(7),
                Constraint::Min(0),
                Constraint::Length(3),
            ]
            .as_ref(),
        )
        .split(area);

    // Manager info block
    let info_text = format!(
        "Name: {}\nCheck Command: {}\nRefresh: {}\nSelf-Update: {}\nUpgrade: {}\nCleanup: {}",
        manager.config.name,
        manager.config.check_command,
        manager.config.refresh.as_deref().unwrap_or("N/A"),
        manager.config.self_update.as_deref().unwrap_or("N/A"),
        manager.config.upgrade_all,
        manager.config.cleanup.as_deref().unwrap_or("N/A")
    );

    let info_block = Paragraph::new(info_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Manager Configuration"),
        )
        .wrap(Wrap { trim: true });

    f.render_widget(info_block, chunks[0]);

    // Status and logs
    let status_color = match manager.status {
        ManagerStatus::Success(_) => Color::Green,
        ManagerStatus::Failed(_) => Color::Red,
        _ => Color::Yellow,
    };

    let status_text = match &manager.status {
        ManagerStatus::Pending => "Status: Pending".to_string(),
        ManagerStatus::Running(operation, _logs) => {
            format!("Status: {}...", operation)
        }
        ManagerStatus::Success(_) => "Status: ✓ All operations completed successfully".to_string(),
        ManagerStatus::Failed(err) => format!("Status: ✗ Failed - {}", err),
    };

    let status_block = Paragraph::new(Text::from(status_text))
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .style(Style::default().fg(status_color))
        .wrap(Wrap { trim: true });

    f.render_widget(status_block, chunks[1]);

    // Help text for detail view
    let help_text = Paragraph::new("Back: Esc/h/← | Logs: l | Quit: q")
        .block(Block::default().borders(Borders::ALL).title("Help"))
        .style(Style::default().fg(Color::Cyan));

    f.render_widget(help_text, chunks[2]);
}

fn render_logs_view(f: &mut Frame, manager: &DetectedManager, scroll_state: &LogsViewState) {
    let area = f.area().inner(Margin {
        horizontal: 2,
        vertical: 1,
    });

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ]
            .as_ref(),
        )
        .split(area);

    // Title block
    let title_text = format!("{} - Live Logs", manager.name);
    let title_block = Paragraph::new(title_text)
        .block(Block::default().borders(Borders::ALL).title("Logs"))
        .style(Style::default().fg(Color::Cyan));

    f.render_widget(title_block, chunks[0]);

    // Raw logs content - show actual package manager output
    let logs_text = match &manager.status {
        ManagerStatus::Running(_operation, logs) => {
            if logs.is_empty() {
                "No output yet...".to_string()
            } else {
                logs.clone()
            }
        }
        ManagerStatus::Success(logs) => {
            if logs.is_empty() {
                "Command completed successfully - no output captured".to_string()
            } else {
                logs.clone()
            }
        }
        ManagerStatus::Failed(err) => err.clone(),
        ManagerStatus::Pending => "Process not started yet...".to_string(),
    };

    let status_color = match manager.status {
        ManagerStatus::Success(_) => Color::Green,
        ManagerStatus::Failed(_) => Color::Red,
        _ => Color::Yellow,
    };

    // Calculate scroll bounds
    let content_height = logs_text.lines().count() as u16;
    let display_height = chunks[1].height.saturating_sub(2); // Subtract borders
    let max_scroll = content_height.saturating_sub(display_height);
    let scroll_offset = scroll_state.scroll_offset.min(max_scroll);

    let logs_block = Paragraph::new(Text::from(logs_text))
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(status_color))
        .wrap(Wrap { trim: true })
        .scroll((scroll_offset, 0));

    f.render_widget(logs_block, chunks[1]);

    // Help text for logs view with scroll indicator
    let scroll_indicator = if content_height > display_height {
        format!(
            " | Scroll: ↑↓/jk PgUp/PgDn Home/End ({}/{})",
            scroll_offset + 1,
            max_scroll + 1
        )
    } else {
        String::new()
    };

    let help_text = Paragraph::new(format!("Back: Esc/h/← | Quit: q{}", scroll_indicator))
        .block(Block::default().borders(Borders::ALL).title("Help"))
        .style(Style::default().fg(Color::Cyan));

    f.render_widget(help_text, chunks[2]);
}

fn print_summary(managers: &[DetectedManager]) {
    let total = managers.len();
    let successful = managers
        .iter()
        .filter(|m| matches!(m.status, ManagerStatus::Success(_)))
        .count();
    let failed = managers
        .iter()
        .filter(|m| matches!(m.status, ManagerStatus::Failed(_)))
        .count();
    let incomplete = total - successful - failed;

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("                           SPINE UPGRADE SUMMARY");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    println!("\nOverall Results:");
    println!("  Total Managers:    {}", total);
    println!(
        "  ✓ Successful:      {} ({:.1}%)",
        successful,
        (successful as f32 / total as f32) * 100.0
    );
    println!(
        "  ✗ Failed:          {} ({:.1}%)",
        failed,
        (failed as f32 / total as f32) * 100.0
    );

    if incomplete > 0 {
        println!(
            "  ? Incomplete:      {} ({:.1}%)",
            incomplete,
            (incomplete as f32 / total as f32) * 100.0
        );
    }

    println!("\nDetailed Results:");
    for manager in managers {
        match &manager.status {
            ManagerStatus::Success(_) => {
                println!("  ✓ {:<20} Success", manager.name);
            }
            ManagerStatus::Failed(err) => {
                println!("  ✗ {:<20} Failed", manager.name);
                println!("    └─ Error: {}", err);
            }
            _ => {
                println!("  ? {:<20} Incomplete", manager.name);
            }
        }
    }

    if failed > 0 {
        println!("\n⚠️  Some package managers failed to upgrade completely.");
        println!("   Check the error details above and consider running 'spn upgrade' again.");
        println!("   You may also need to run the failed managers manually with sudo privileges.");
    } else if successful > 0 {
        println!("\n🎉 All package managers upgraded successfully!");
        println!("   Your system is now up to date.");
    }

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
}
