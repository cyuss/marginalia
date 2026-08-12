//! Drawing. No decisions are made here.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::action::{Danger, Terminal as ActionTerminal};
use crate::app::{App, Mode};

pub fn draw(frame: &mut Frame, app: &App) {
    let areas = Layout::vertical([
        Constraint::Length(3), // title
        Constraint::Min(6),    // menu
        Constraint::Length(5), // what the highlighted entry does
        Constraint::Length(1), // keys
    ])
    .split(frame.area());

    draw_title(frame, areas[0], app);
    draw_menu(frame, areas[1], app);
    draw_explanation(frame, areas[2], app);
    draw_keys(frame, areas[3], app);

    match app.mode {
        Mode::EditingHost => draw_host_editor(frame, app),
        Mode::Confirming => draw_confirmation(frame, app),
        Mode::Output => draw_output(frame, app),
        Mode::Browsing => {}
    }
}

fn draw_title(frame: &mut Frame, area: Rect, app: &App) {
    let line = Line::from(vec![
        Span::styled("Marginalia", Style::new().bold()),
        Span::raw("   device "),
        Span::styled(&app.host, Style::new().fg(Color::Cyan)),
        Span::styled("   (h to change)", Style::new().dim()),
    ]);
    frame.render_widget(
        Paragraph::new(line).block(Block::default().borders(Borders::BOTTOM)),
        area,
    );
}

fn draw_menu(frame: &mut Frame, area: Rect, app: &App) {
    let entries = app.entries();
    let mut items: Vec<ListItem> = Vec::with_capacity(entries.len());
    let mut last_section = "";

    for (section, action) in &entries {
        // A heading is drawn as part of the first item of its section, so the
        // list's own indices still line up with `app.selected`.
        let mut lines = Vec::new();
        if section.title != last_section {
            lines.push(Line::from(Span::styled(
                section.title.to_uppercase(),
                Style::new().dim(),
            )));
            last_section = section.title;
        }

        let marker = match action.danger {
            Danger::Removes => Span::styled("  ! ", Style::new().fg(Color::Red)),
            Danger::Ordinary => Span::raw("    "),
        };
        lines.push(Line::from(vec![marker, Span::raw(action.label)]));
        items.push(ListItem::new(lines));
    }

    let mut state = ListState::default().with_selected(Some(app.selected));
    frame.render_stateful_widget(
        List::new(items).highlight_style(Style::new().bold().bg(Color::DarkGray)),
        area,
        &mut state,
    );
}

fn draw_explanation(frame: &mut Frame, area: Rect, app: &App) {
    let text = match app.current() {
        Some(action) => {
            let mut lines = vec![Line::from(Span::raw(action.summary))];
            if action.terminal == ActionTerminal::HandOver {
                lines.push(Line::from(Span::styled(
                    "Runs in this terminal, so you can answer any prompt yourself.",
                    Style::new().dim(),
                )));
            }
            lines.push(Line::from(Span::styled(
                format!("$ {}", first_line(action.command)),
                Style::new().fg(Color::DarkGray),
            )));
            if !app.status.is_empty() {
                lines.push(Line::from(Span::styled(
                    app.status.as_str(),
                    Style::new().fg(Color::Yellow),
                )));
            }
            Text::from(lines)
        }
        None => Text::raw(""),
    };

    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::TOP)),
        area,
    );
}

fn draw_keys(frame: &mut Frame, area: Rect, app: &App) {
    let keys = match app.mode {
        Mode::Browsing => "↑↓ move   ⏎ run   h device address   q quit",
        Mode::EditingHost => "⏎ save   esc cancel",
        Mode::Confirming => "y run it   n / esc cancel",
        Mode::Output => "any key to go back",
    };
    frame.render_widget(Paragraph::new(Span::styled(keys, Style::new().dim())), area);
}

fn draw_host_editor(frame: &mut Frame, app: &App) {
    let area = centred(frame.area(), 56, 7);
    frame.render_widget(Clear, area);
    let body = Text::from(vec![
        Line::from(Span::raw(app.host_draft.as_str())),
        Line::from(""),
        Line::from(Span::styled(
            "10.11.99.1 over USB, or the Wi-Fi address from",
            Style::new().dim(),
        )),
        Line::from(Span::styled("Settings → Help → About", Style::new().dim())),
    ]);
    frame.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Device address "),
        ),
        area,
    );
}

fn draw_confirmation(frame: &mut Frame, app: &App) {
    let area = centred(frame.area(), 62, 9);
    frame.render_widget(Clear, area);
    let label = app.current().map(|a| a.label).unwrap_or("");
    let body = Text::from(vec![
        Line::from(Span::styled(label, Style::new().bold())),
        Line::from(""),
        Line::from(Span::raw("This removes things. Run it?")),
        Line::from(""),
        Line::from(Span::styled(
            "You will still be asked to type `remove` afterwards.",
            Style::new().dim(),
        )),
        Line::from(Span::styled(
            "Your documents are never touched.",
            Style::new().dim(),
        )),
    ]);
    frame.render_widget(
        Paragraph::new(body).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::new().fg(Color::Red))
                .title(" Are you sure "),
        ),
        area,
    );
}

fn draw_output(frame: &mut Frame, app: &App) {
    let full = frame.area();
    let area = centred(
        full,
        full.width.saturating_sub(6),
        full.height.saturating_sub(4),
    );
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(app.output.as_str())
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(" Output ")),
        area,
    );
}

/// First line only: some commands are several lines, and a menu is not the
/// place to print a shell script.
fn first_line(command: &str) -> String {
    let one_line: String = command.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > 96 {
        format!("{}…", one_line.chars().take(95).collect::<String>())
    } else {
        one_line
    }
}

fn centred(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_command_is_shortened_to_fit_a_line() {
        let long = "a ".repeat(200);
        assert!(first_line(&long).chars().count() <= 96);
    }

    #[test]
    fn a_multi_line_command_becomes_one_line() {
        assert_eq!(
            first_line("echo one \\\n  && echo two"),
            "echo one \\ && echo two"
        );
    }

    #[test]
    fn a_box_never_escapes_the_screen_it_is_centred_in() {
        let screen = Rect::new(0, 0, 40, 10);
        let area = centred(screen, 100, 100);
        assert!(area.x + area.width <= screen.width);
        assert!(area.y + area.height <= screen.height);
    }
}
