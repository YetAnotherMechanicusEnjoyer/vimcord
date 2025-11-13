use ratatui::{
    style::{Style, Stylize},
    widgets::{List, ListItem, ListState},
};
use unicode_width::UnicodeWidthStr;

use crate::{App, AppState, model::message::Message};

pub fn draw_ui(f: &mut ratatui::Frame, app: &mut App) {
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::text::{Line, Text};
    use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(90), Constraint::Percentage(10)].as_ref())
        .split(area);

    app.terminal_height = chunks[0].height as usize;
    app.terminal_width = chunks[0].width as usize;

    let max_height = app.terminal_height.saturating_sub(2);
    let max_width = app.terminal_width.saturating_sub(2) as u16;

    match &app.state {
        AppState::SelectingGuild => {
            let items: Vec<ListItem> = app
                .guilds
                .iter()
                .map(|g| ListItem::new(g.name.as_str()))
                .collect();

            let list = List::new(items)
                .block(
                    Block::default()
                        .title("Servers (Guilds)")
                        .borders(Borders::ALL),
                )
                .highlight_style(Style::default().reversed())
                .highlight_symbol(">> ");

            let mut state = ListState::default().with_selected(Some(app.selection_index));
            f.render_stateful_widget(list, chunks[0], &mut state);
        }
        AppState::SelectingChannel(guild_id) => {
            let title = format!("Channels for Guild: {guild_id}");
            let items: Vec<ListItem> = app
                .channels
                .iter()
                .filter(|c| c.channel_type != 4)
                .map(|c| {
                    let char = match c.channel_type {
                        2 => '',
                        _ => '',
                    };

                    ListItem::new(format!("{char} {}", c.name))
                })
                .collect();

            let list = List::new(items)
                .block(Block::default().title(title).borders(Borders::ALL))
                .highlight_style(Style::default().reversed())
                .highlight_symbol(">> ");

            let mut state = ListState::default().with_selected(Some(app.selection_index));
            f.render_stateful_widget(list, chunks[0], &mut state);
        }
        AppState::Chatting(_) => {
            if max_width == 0 {
                return;
            }

            let mut messages_to_render: Vec<Message> = Vec::new();
            let mut current_height = 0;

            for message in app.messages.iter() {
                let formatted_text = format!(
                    "[{}] {}: {}",
                    message
                        .timestamp
                        .split('T')
                        .nth(1)
                        .unwrap_or("")
                        .split('.')
                        .next()
                        .unwrap_or(""),
                    message.author.username,
                    message.content.as_deref().unwrap_or("(*non-text*)")
                );

                let text_lines: Vec<&str> = formatted_text.split('\n').collect();
                let mut estimated_height = 0;

                for line in text_lines {
                    let width = UnicodeWidthStr::width(line) as u16;

                    if width == 0 {
                        estimated_height += 1;
                        continue;
                    }

                    #[allow(clippy::manual_div_ceil)]
                    let wrap_lines =
                        (width as usize + max_width as usize - 1) / (max_width as usize);

                    estimated_height += wrap_lines;
                }

                if current_height + estimated_height > max_height {
                    break;
                }

                current_height += estimated_height;

                messages_to_render.push(message.clone());
            }

            messages_to_render.reverse();

            let mut final_content: Vec<Line> = Vec::new();

            for message in messages_to_render.into_iter() {
                let formatted_text = format!(
                    "[{}] {}: {}",
                    message
                        .timestamp
                        .split('T')
                        .nth(1)
                        .unwrap_or("")
                        .split('.')
                        .next()
                        .unwrap_or(""),
                    message.author.username,
                    message.content.as_deref().unwrap_or("(*non-text*)")
                );
                let text = Text::raw(formatted_text);

                final_content.extend(text.lines);
            }

            let scroll_offset = if final_content.len() > max_height {
                final_content.len().saturating_sub(max_height)
            } else {
                0
            };

            let paragraph = Paragraph::new(final_content)
                .block(
                    Block::default()
                        .title("Rivet Client (Esc to return to Servers")
                        .borders(Borders::ALL),
                )
                .wrap(Wrap { trim: false })
                .scroll((scroll_offset as u16, 0));

            f.render_widget(paragraph, chunks[0]);
        }
    };

    f.render_widget(
        Paragraph::new(app.input.as_str()).block(
            Block::default()
                .title(format!("Input: {}", app.status_message))
                .borders(Borders::ALL),
        ),
        chunks[1],
    )
}
