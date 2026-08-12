//! # The Marginalia terminal interface
//!
//! Runs on your computer, not on the reMarkable. It is the place to install,
//! check, configure and remove Marginalia without remembering commands.
//!
//! ## What it is not
//!
//! It is not the product. The product is the reMarkable's own reader, which
//! Marginalia leaves alone — see
//! `docs/adr/ADR-002-remarkable-ui-and-runtime.md`. This is a front door to the
//! scripts in `tools/device/` and to the agent, nothing more, and it is
//! deliberately possible to do everything here by typing the commands yourself.
//!
//! ## Why it hands the terminal over
//!
//! Every device action needs a password prompt, and removal needs you to type
//! `remove`. So instead of capturing output, the interface steps out of the
//! way: it leaves the alternate screen, runs the command attached to your real
//! terminal, waits, and comes back. You answer the prompts. Nothing here ever
//! answers a safety question on your behalf.

use std::io::{self, Stdout};
use std::process::Command;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::*;

mod action;
mod app;
mod ui;

use app::{App, Mode, DEFAULT_HOST};

fn main() -> io::Result<()> {
    let host = std::env::var("RM_HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string());
    let mut app = App::new(host);

    let mut terminal = enter()?;
    let result = run(&mut terminal, &mut app);
    leave(&mut terminal)?;
    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        if let Some(action) = app.pending_handover.take() {
            match action.terminal {
                action::Terminal::HandOver => {
                    // Out of the interface, into the user's terminal, back again.
                    leave(terminal)?;
                    let status = shell(action.command, &app.host);
                    pause_for_the_reader(&status);
                    *terminal = enter()?;
                    app.status = status;
                }
                action::Terminal::Capture => {
                    let output = capture(action.command, &app.host);
                    app.show_output(output);
                }
            }
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match app.mode {
            Mode::Browsing => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                KeyCode::Char('h') => app.begin_editing_host(),
                KeyCode::Enter => app.activate(),
                _ => {}
            },
            Mode::EditingHost => match key.code {
                KeyCode::Enter => app.commit_host(),
                KeyCode::Esc => app.mode = Mode::Browsing,
                KeyCode::Backspace => {
                    app.host_draft.pop();
                }
                KeyCode::Char(c) => app.host_draft.push(c),
                _ => {}
            },
            // Only `y` runs it. Any other key is a no.
            Mode::Confirming => match key.code {
                KeyCode::Char('y') => app.confirm(),
                _ => app.cancel(),
            },
            Mode::Output => app.dismiss_output(),
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

/// Run a command in the user's own terminal, inheriting stdin and stdout.
///
/// `RM_HOST` is passed through the environment rather than substituted into the
/// command string, so an address containing shell metacharacters cannot become
/// part of the command.
fn shell(command: &str, host: &str) -> String {
    match Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("RM_HOST", host)
        .status()
    {
        Ok(status) if status.success() => "Finished.".to_string(),
        Ok(status) => format!(
            "Stopped with status {}. Nothing else was attempted.",
            status
        ),
        Err(e) => format!("Could not start it: {e}"),
    }
}

fn capture(command: &str, host: &str) -> String {
    match Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("RM_HOST", host)
        .output()
    {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            if !out.stderr.is_empty() {
                text.push_str(&String::from_utf8_lossy(&out.stderr));
            }
            text
        }
        Err(e) => format!("Could not start it: {e}"),
    }
}

/// The output of the command that just ran is on screen; redrawing over it
/// immediately would throw away the thing the user asked to see.
fn pause_for_the_reader(status: &str) {
    println!("\n{status}");
    println!("Press enter to go back.");
    let mut sink = String::new();
    let _ = io::stdin().read_line(&mut sink);
}

fn enter() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(out))
}

fn leave(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_runs_and_reports_success() {
        assert_eq!(capture("printf hello", "10.11.99.1"), "hello");
    }

    /// The address reaches the command as an environment variable, so quoting
    /// it in the command string is the caller's only job.
    #[test]
    fn the_device_address_is_passed_through_the_environment() {
        assert_eq!(
            capture("printf %s \"$RM_HOST\"", "192.168.1.42"),
            "192.168.1.42"
        );
    }

    /// An address is data, never syntax. If it were substituted into the
    /// command string, this would run `touch` instead of printing.
    #[test]
    fn an_address_containing_shell_syntax_cannot_become_a_command() {
        let hostile = "1.2.3.4; echo INJECTED";
        let out = capture("printf %s \"$RM_HOST\"", hostile);
        assert_eq!(out, hostile);
        assert!(!out.contains("INJECTED\n"));
    }

    #[test]
    fn a_failure_is_reported_rather_than_swallowed() {
        let status = shell("exit 3", "10.11.99.1");
        assert!(status.contains("Stopped"), "got: {status}");
    }
}
