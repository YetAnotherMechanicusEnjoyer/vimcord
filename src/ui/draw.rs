use ratatui::{
    style::{Color, Style, Stylize},
    text::Span,
    widgets::{BorderType, Clear, List, ListItem, ListState},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    App, AppState,
    api::{Channel, DM, Emoji, Guild, Message},
};

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
        AppState::Loading(_) => {
            let loading_area = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(40),
                    Constraint::Length(3),
                    Constraint::Min(0),
                ])
                .split(chunks[0])[1];

            let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let symbol = spinner[app.tick_count % spinner.len()];

            let loading_text = Line::from(vec![
                Span::styled("Loading", Style::default().fg(Color::LightCyan)),
                Span::raw(" "),
                Span::styled(symbol, Style::default().fg(Color::LightCyan)),
            ]);

            let loading_paragraph = Paragraph::new(Text::from(vec![loading_text]))
                .alignment(ratatui::layout::Alignment::Center)
                .block(Block::default().borders(Borders::NONE));

            f.render_widget(Clear, chunks[0]);
            f.render_widget(loading_paragraph, loading_area);
        }
        AppState::Home => {
            let options = [
                ("Guilds", Color::LightMagenta),
                ("DMs", Color::LightYellow),
                ("Quit", Color::LightRed),
            ];

            let items: Vec<ListItem> = options.iter().map(|o| ListItem::new(o.0).fg(o.1)).collect();

            let list = List::new(items)
                .block(
                    Block::default()
                        .title(Span::styled(
                            "Rivet Client - Home",
                            Style::default().fg(Color::Yellow),
                        ))
                        .borders(Borders::ALL)
                        .border_type(BorderType::Double),
                )
                .highlight_style(Style::default().reversed())
                .highlight_symbol(">> ");

            app.selection_index = app.selection_index.min(options.len().saturating_sub(1));

            let mut state = ListState::default().with_selected(Some(app.selection_index));
            f.render_widget(Clear, chunks[0]);
            f.render_stateful_widget(list, chunks[0], &mut state);
        }
        AppState::SelectingDM => {
            let filter_text = app.input.to_lowercase();

            let filtered_dms: Vec<&DM> = app
                .dms
                .iter()
                .filter(|d| d.get_name().to_lowercase().contains(&filter_text))
                .collect();

            let items: Vec<ListItem> = filtered_dms
                .iter()
                .map(|d| {
                    let char = match d.channel_type {
                        1 => '',
                        3 => '',
                        _ => '',
                    };

                    let color = match d.channel_type {
                        1 => Color::LightMagenta,
                        3 => Color::LightBlue,
                        _ => Color::LightRed,
                    };

                    ListItem::new(format!("{char} {}", d.get_name()))
                        .style(Style::default().fg(color))
                })
                .collect();

            let num_filtered = items.len();
            app.selection_index = app.selection_index.min(num_filtered.saturating_sub(1));

            let list = List::new(items)
                .block(
                    Block::default()
                        .title(Span::styled(
                            "Rivet Client - Direct Messages",
                            Style::default().fg(Color::Yellow),
                        ))
                        .borders(Borders::ALL)
                        .border_type(BorderType::Double),
                )
                .highlight_style(Style::default().reversed())
                .highlight_symbol(">> ");

            let mut state = ListState::default().with_selected(Some(app.selection_index));
            f.render_widget(Clear, chunks[0]);
            f.render_stateful_widget(list, chunks[0], &mut state);
        }
        AppState::SelectingGuild => {
            let filter_text = app.input.to_lowercase();

            let filtered_guilds: Vec<&Guild> = app
                .guilds
                .iter()
                .filter(|g| g.name.to_lowercase().contains(&filter_text))
                .collect();

            let mut count = 0;
            let items: Vec<ListItem> = filtered_guilds
                .iter()
                .map(|g| {
                    let color = if count % 2 == 0 {
                        Color::LightCyan
                    } else {
                        Color::LightYellow
                    };

                    count += 1;

                    ListItem::new(g.name.as_str()).style(Style::default().fg(color))
                })
                .collect();

            let num_filtered = items.len();
            app.selection_index = app.selection_index.min(num_filtered.saturating_sub(1));

            let list = List::new(items)
                .block(
                    Block::default()
                        .title(Span::styled(
                            "Rivet Client - Guilds",
                            Style::default().fg(Color::Yellow),
                        ))
                        .borders(Borders::ALL)
                        .border_type(BorderType::Double),
                )
                .highlight_style(Style::default().reversed())
                .highlight_symbol(">> ");

            let mut state = ListState::default().with_selected(Some(app.selection_index));
            f.render_widget(Clear, chunks[0]);
            f.render_stateful_widget(list, chunks[0], &mut state);
        }
        AppState::SelectingChannel(guild_id) => {
            let filter_text = app.input.to_lowercase();

            let permission_context = &app.context;

            let mut list_items: Vec<ListItem> = Vec::new();

            let should_display_channel_content = |c: &Channel| {
                let is_readable = permission_context
                    .as_ref()
                    .is_some_and(|context| c.is_readable(context));

                is_readable
                    && (filter_text.is_empty() || c.name.to_lowercase().contains(&filter_text))
            };

            app.channels
                .iter()
                .filter(|c| {
                    if c.children.is_none() && c.channel_type != 4 {
                        return should_display_channel_content(c);
                    }

                    if c.channel_type == 4 {
                        if filter_text.is_empty() || c.name.to_lowercase().contains(&filter_text) {
                            return true;
                        }

                        if let Some(children) = &c.children {
                            return children.iter().any(should_display_channel_content);
                        }
                    }

                    false
                })
                .for_each(|c| {
                    let get_channel_style = |channel_type: u8| -> (char, Color) {
                        match channel_type {
                            15 => ('', Color::LightYellow),
                            13 => ('󱝉', Color::LightRed),
                            5 => ('', Color::LightGreen),
                            4 => ('', Color::Gray),
                            2 => ('', Color::LightCyan),
                            0 => ('', Color::LightBlue),
                            _ => ('', Color::LightMagenta),
                        }
                    };

                    if c.channel_type == 4 {
                        let (char, color) = get_channel_style(c.channel_type);
                        list_items.push(
                            ListItem::new(format!("{char} {}", c.name))
                                .style(Style::default().fg(color)),
                        );

                        if let Some(children) = &c.children {
                            children
                                .iter()
                                .filter(|c| should_display_channel_content(c))
                                .for_each(|child| {
                                    let (char, color) = get_channel_style(child.channel_type);

                                    list_items.push(
                                        ListItem::new(format!("  {char} {}", child.name))
                                            .style(Style::default().fg(color)),
                                    );
                                });
                        }
                    } else {
                        let (char, color) = get_channel_style(c.channel_type);
                        list_items.push(
                            ListItem::new(format!("{char} {}", c.name))
                                .style(Style::default().fg(color)),
                        );
                    }
                });

            let num_filtered = list_items.len();
            app.selection_index = app.selection_index.min(num_filtered.saturating_sub(1));

            let hidden_items: Vec<ListItem> = app
                .channels
                .iter()
                .flat_map(|c| {
                    if c.channel_type == 4 {
                        let mut items: Vec<&Channel> = Vec::new();

                        if let Some(children) = &c.children {
                            items.extend(children.iter().filter(|child| {
                                permission_context
                                    .as_ref()
                                    .is_some_and(|context| !child.is_readable(context))
                            }));
                        }
                        items
                    } else if permission_context
                        .as_ref()
                        .is_some_and(|context| !c.is_readable(context))
                    {
                        vec![c]
                    } else {
                        vec![]
                    }
                })
                .map(|c| {
                    let char = match c.channel_type {
                        15 => '',
                        13 => '󱝉',
                        5 => '',
                        4 => '',
                        2 => '',
                        0 => '',
                        _ => '',
                    };

                    let color = Color::DarkGray;

                    ListItem::new(format!(" {char} {}", c.name)).style(Style::default().fg(color))
                })
                .collect();

            list_items.extend(hidden_items);

            let title = format!(
                "Channels for Guild: {guild_id} | Channels found: {} | Actual index: {}",
                num_filtered.saturating_sub(1),
                app.selection_index
            );

            let list = List::new(list_items)
                .block(
                    Block::default()
                        .title(Span::styled(title, Style::default().fg(Color::Yellow)))
                        .borders(Borders::ALL)
                        .border_type(BorderType::Double),
                )
                .highlight_style(Style::default().reversed())
                .highlight_symbol(">> ");

            let mut state = ListState::default().with_selected(Some(app.selection_index));
            f.render_widget(Clear, chunks[0]);
            f.render_stateful_widget(list, chunks[0], &mut state);
        }
        AppState::Chatting(_) | AppState::EmojiSelection(_) => {
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

                    if width == 0 || max_width == 0 {
                        estimated_height += 1;
                        continue;
                    }

                    let wrap_lines = (width as usize).div_ceil(max_width as usize);

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
                let mut lines = vec![];

                let formatted_time = format!(
                    " {}]",
                    message
                        .timestamp
                        .split('T')
                        .nth(1)
                        .unwrap_or("")
                        .split('.')
                        .next()
                        .unwrap_or(""),
                );

                let formatted_date = message
                    .timestamp
                    .split('T')
                    .next()
                    .unwrap_or("")
                    .to_string();

                let author = format!(" {}: ", message.author.username);

                let content = message
                    .content
                    .clone()
                    .unwrap_or("(*non-text*)".to_string());

                lines.push(Line::from(vec![
                    Span::styled("[".to_string(), Style::default().fg(Color::LightBlue)),
                    Span::styled(formatted_date, Style::default().fg(Color::LightCyan)),
                    Span::styled(formatted_time, Style::default().fg(Color::LightBlue)),
                    Span::styled(author, Style::default().fg(Color::Yellow)),
                    Span::styled(content, Style::default().fg(Color::White)),
                ]));

                let text = Text::from(lines);

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
                        .title(Span::styled(
                            "Rivet Client - Chatting",
                            Style::default().fg(Color::Yellow),
                        ))
                        .borders(Borders::ALL)
                        .border_type(BorderType::Double),
                )
                .wrap(Wrap { trim: false })
                .scroll((scroll_offset as u16, 0));

            f.render_widget(Clear, chunks[0]);
            f.render_widget(paragraph, chunks[0]);
        }
    };

    if let AppState::EmojiSelection(_) = &app.state {
        let input_area = chunks[1];
        let emoji_popup_height = 8;

        let popup_rect = ratatui::layout::Rect {
            x: input_area.x + 1,
            y: input_area.y.saturating_sub(emoji_popup_height + 1),
            width: input_area.width.saturating_sub(2),
            height: emoji_popup_height,
        };

        f.render_widget(Clear, popup_rect);

        let mut filtered_items: Vec<ListItem> = Vec::new();

        let app_clone = app.clone();

        let filtered_unicode: Vec<&(String, String)> = app_clone
            .emoji_map
            .iter()
            .filter(|(name, _)| name.starts_with(&app.emoji_filter))
            .collect();

        let filtered_custom: Vec<&Emoji> = app_clone
            .custom_emojis
            .iter()
            .filter(|e| e.name.starts_with(&app.emoji_filter))
            .collect();

        for (name, char) in filtered_unicode.iter() {
            filtered_items.push(ListItem::new(Line::from(vec![
                Span::styled(char.clone(), Style::default().fg(Color::White)),
                Span::raw(" "),
                Span::styled(
                    format!(":{name}: (Unicode)"),
                    Style::default().fg(Color::LightBlue),
                ),
            ])));
        }

        for emoji in filtered_custom.iter() {
            filtered_items.push(ListItem::new(Line::from(vec![Span::styled(
                format!("  :{}: (Guild)", emoji.name),
                Style::default().fg(Color::LightBlue),
            )])));
        }

        if !filtered_items.is_empty() {
            app.selection_index = app
                .selection_index
                .min(filtered_items.len().saturating_sub(1));

            let emoji_list = List::new(filtered_items)
                .block(
                    Block::default()
                        .title(Span::styled(
                            "Select An Emoji",
                            Style::default().fg(Color::Yellow),
                        ))
                        .borders(Borders::ALL)
                        .border_type(BorderType::Double),
                )
                .highlight_style(Style::default().reversed())
                .highlight_symbol(">> ");

            let mut state = ListState::default().with_selected(Some(app.selection_index));
            f.render_stateful_widget(emoji_list, popup_rect, &mut state);
        } else {
            app.selection_index = 0;
        }
    }

    f.render_widget(
        Paragraph::new(app.input.as_str()).block(
            Block::default()
                .title(Span::styled(
                    format!("Input: {}", app.status_message),
                    Style::default().fg(Color::Yellow),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Double),
        ),
        chunks[1],
    );

    let cursor_x = chunks[1].x + 1 + app.input.width() as u16;
    let cursor_y = chunks[1].y + 1;
    f.set_cursor_position((cursor_x, cursor_y));
}
