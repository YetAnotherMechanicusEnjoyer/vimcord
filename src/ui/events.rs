use std::io;

use crossterm::event::{self, KeyCode, KeyEventKind};
use tokio::{
    sync::{MutexGuard, mpsc::Sender},
    time::{self, Duration},
};

use crate::{
    App, AppAction, AppState, InputMode, KeywordAction, Window,
    api::{Channel, DM, Emoji, Guild},
    ui::vim,
};

/// Helper function to insert a character at the cursor position.
/// Handles both emoji selection state and normal input state.
fn insert_char_at_cursor(state: &mut MutexGuard<'_, App>, c: char) {
    let current_state = state.state.clone();
    match current_state {
        AppState::EmojiSelection(channel_id) => {
            let pos = state.cursor_position;
            state.input.insert(pos, c);
            state.cursor_position += c.len_utf8();
            if c == ' ' {
                state.state = AppState::Chatting(channel_id.clone());
                state.emoji_filter.clear();
                state.emoji_filter_start = None;
            } else {
                // Recompute emoji_filter based on the current input and emoji_filter_start.
                if let Some(start) = state.emoji_filter_start {
                    let filter_start = start + ':'.len_utf8();
                    if state.cursor_position <= start || filter_start > state.input.len() {
                        state.emoji_filter.clear();
                    } else {
                        let end = std::cmp::min(state.cursor_position, state.input.len());
                        if filter_start <= end {
                            state.emoji_filter = state.input[filter_start..end].to_string();
                        } else {
                            state.emoji_filter.clear();
                        }
                    }
                } else {
                    state.emoji_filter.clear();
                }

                if state.emoji_filter.is_empty() {
                    state.state = AppState::Chatting(channel_id.clone());
                    state.emoji_filter_start = None;
                    state.status_message =
                        "Chatting in channel. Press Enter to send message. Esc to return channels"
                            .to_string();
                }
            }
            state.selection_index = 0;
        }
        _ => {
            let pos = state.cursor_position;
            state.input.insert(pos, c);
            state.cursor_position += c.len_utf8();
        }
    }
}

pub async fn handle_input_events(
    tx: Sender<AppAction>,
    mut rx_shutdown: tokio::sync::broadcast::Receiver<()>,
) -> Result<(), io::Error> {
    loop {
        tokio::select! {
            _ = rx_shutdown.recv() => {
                return Ok(());
            }

            _ = time::sleep(Duration::from_millis(10)) => {
                if event::poll(Duration::from_millis(0))? {
                    match event::read()? {
                        event::Event::Key(key) => {
                            if key.kind == KeyEventKind::Press {
                                if key.code == KeyCode::Char('c') && key.modifiers.contains(event::KeyModifiers::CONTROL) {
                                    tx.send(AppAction::SigInt).await.ok();
                                } else {
                                    match key.code {
                                        KeyCode::Esc => {
                                            tx.send(AppAction::InputEscape).await.ok();
                                        }
                                        KeyCode::Enter => {
                                            tx.send(AppAction::InputSubmit).await.ok();
                                        }
                                        KeyCode::Backspace => {
                                            tx.send(AppAction::InputBackspace).await.ok();
                                        }
                                        KeyCode::Up => {
                                            tx.send(AppAction::SelectPrevious).await.ok();
                                        }
                                        KeyCode::Down => {
                                            tx.send(AppAction::SelectNext).await.ok();
                                        }
                                        KeyCode::Char(c) => {
                                            tx.send(AppAction::InputChar(c)).await.ok();
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        event::Event::Paste(s) => {
                            tx.send(AppAction::Paste(s)).await.ok();
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

async fn input_submit(
    state: &mut MutexGuard<'_, App>,
    tx_action: &Sender<AppAction>,
    filtered_unicode: Vec<&(String, String)>,
    filtered_custom: Vec<&Emoji>,
    total_filtered_emojis: usize,
) -> Option<KeywordAction> {
    match &state.clone().state {
        AppState::Loading(_) => {}
        AppState::Home => match state.selection_index {
            0 => {
                tx_action.send(AppAction::TransitionToGuilds).await.ok();
            }
            1 => {
                tx_action.send(AppAction::TransitionToDM).await.ok();
            }
            2 => {
                return Some(KeywordAction::Break);
            }
            _ => {}
        },
        AppState::SelectingDM => {
            let filter_text = state.input.to_lowercase();
            let dms: Vec<&DM> = state
                .dms
                .iter()
                .filter(|d| d.get_name().to_lowercase().contains(&filter_text))
                .collect();

            if dms.is_empty() {
                return Some(KeywordAction::Continue);
            }

            let selected_dm = &dms[state.selection_index];
            let dm_id_clone = selected_dm.id.clone();
            let selected_dm_name = selected_dm.recipients[0].username.clone();

            state.input = String::new();
            state.cursor_position = 0;
            state.status_message = format!("Loading messages for {selected_dm_name}...");

            tx_action
                .send(AppAction::TransitionToChat(dm_id_clone))
                .await
                .ok();
        }
        AppState::SelectingGuild => {
            let filter_text = state.input.to_lowercase();
            let guilds: Vec<&Guild> = state
                .guilds
                .iter()
                .filter(|g| g.name.to_lowercase().contains(&filter_text))
                .collect();

            if guilds.is_empty() {
                return Some(KeywordAction::Continue);
            }

            let selected_guild = &guilds[state.selection_index];
            let guild_id_clone = selected_guild.id.clone();
            let selected_guild_name = selected_guild.name.clone();

            let tx_clone = tx_action.clone();

            state.status_message = format!("Loading channels for {selected_guild_name}...");

            let api_client_clone = state.api_client.clone();

            tokio::spawn(async move {
                tx_clone
                    .send(AppAction::TransitionToLoading(Window::Channel(
                        guild_id_clone.clone(),
                    )))
                    .await
                    .ok();
                match api_client_clone.get_guild_channels(&guild_id_clone).await {
                    Ok(channels) => {
                        tx_clone
                            .send(AppAction::ApiUpdateChannel(channels))
                            .await
                            .ok();
                    }
                    Err(e) => {
                        eprintln!("Failed to load channels: {e}");
                    }
                }
                match api_client_clone.get_guild_emojis(&guild_id_clone).await {
                    Ok(emojis) => {
                        tx_clone.send(AppAction::ApiUpdateEmojis(emojis)).await.ok();
                    }
                    Err(e) => {
                        eprintln!("Failed to load custom emojis: {e}");
                    }
                }
                match api_client_clone
                    .get_permission_context(&guild_id_clone)
                    .await
                {
                    Ok(context) => {
                        tx_clone
                            .send(AppAction::ApiUpdateContext(Some(context)))
                            .await
                            .ok();
                    }
                    Err(e) => {
                        eprintln!("Failed to load permission context: {e}");
                    }
                }

                tx_clone.send(AppAction::EndLoading).await.ok();
            });
        }
        AppState::SelectingChannel(_) => {
            let permission_context = &state.context;
            let mut text_channels: Vec<&Channel> = Vec::new();

            let filter_text = state.input.to_lowercase();
            state
                .channels
                .iter()
                .filter(|c| {
                    let mut readable = false;
                    if let Some(context) = &permission_context {
                        readable = c.is_readable(context)
                    }
                    readable && c.name.to_lowercase().contains(&filter_text)
                })
                .for_each(|c| {
                    if let Some(children) = &c.children {
                        text_channels.push(c);

                        children
                            .iter()
                            .filter(|c| {
                                let mut readable = false;
                                if let Some(context) = &permission_context {
                                    readable = c.is_readable(context)
                                }
                                readable && c.name.to_lowercase().contains(&filter_text)
                            })
                            .for_each(|c| {
                                text_channels.push(c);
                            });
                    } else {
                        text_channels.push(c);
                    }
                });

            if text_channels.is_empty()
                || text_channels.len() <= state.selection_index
                || text_channels[state.selection_index].channel_type == 4
            {
                return Some(KeywordAction::Continue);
            }

            let channel_info = {
                let selected_channel = &text_channels[state.selection_index];
                (selected_channel.id.clone(), selected_channel.name.clone())
            };
            let (channel_id_clone, selected_channel_name) = channel_info;

            tx_action
                .send(AppAction::TransitionToLoading(Window::Chat(
                    channel_id_clone.clone(),
                )))
                .await
                .ok();

            state.input = String::new();
            state.cursor_position = 0;
            state.status_message = format!("Loading messages for {selected_channel_name}...");

            match state
                .api_client
                .get_channel_messages(&channel_id_clone, None, None, None, Some(100))
                .await
            {
                Ok(messages) => {
                    if let Err(e) = tx_action.send(AppAction::ApiUpdateMessages(messages)).await {
                        eprintln!("Failed to send message update action: {e}");
                        return None;
                    }
                }
                Err(e) => {
                    state.status_message = format!("Error loading chat: {e}");
                }
            }

            tx_action.send(AppAction::EndLoading).await.ok();
        }
        AppState::EmojiSelection(channel_id) => {
            let start_pos = state.emoji_filter_start?;
            let end_pos = start_pos + ':'.len_utf8() + state.emoji_filter.len();

            if state.selection_index < filtered_unicode.len() {
                let (_, char) = filtered_unicode[state.selection_index];

                if state.input.is_char_boundary(start_pos) && state.input.is_char_boundary(end_pos)
                {
                    state.input.drain(start_pos..end_pos);

                    state.input.insert_str(start_pos, char);
                    let mut pos = start_pos + char.len();
                    state.input.insert(pos, ' ');
                    pos += ' '.len_utf8();

                    state.cursor_position = pos;
                }
            } else if state.selection_index < total_filtered_emojis {
                let custom_index = state.selection_index - filtered_unicode.len();
                let emoji = filtered_custom[custom_index];

                let emoji_string = format!(
                    "<{}:{}:{}>",
                    if emoji.animated.unwrap_or(false) {
                        "a"
                    } else {
                        ""
                    },
                    emoji.name,
                    emoji.id
                );

                if state.input.is_char_boundary(start_pos) && state.input.is_char_boundary(end_pos)
                {
                    state.input.drain(start_pos..end_pos);

                    state.input.insert_str(start_pos, &emoji_string);
                    let mut pos = start_pos + emoji_string.len();
                    state.input.insert(pos, ' ');
                    pos += ' '.len_utf8();

                    state.cursor_position = pos;
                }
            }

            state.state = AppState::Chatting(channel_id.clone());
            state.emoji_filter.clear();
            state.emoji_filter_start = None;
            state.selection_index = 0;
            state.status_message =
                "Chatting in channel. Press Enter to send message, Esc to return to channels."
                    .to_string();
        }
        AppState::Chatting(_) => {
            let channel_id_clone = if let AppState::Chatting(id) = &state.state {
                Some(id.clone())
            } else {
                None
            };

            let content = state.input.drain(..).collect::<String>();
            state.cursor_position = 0;

            let message_data = if content.is_empty() || channel_id_clone.is_none() {
                None
            } else {
                channel_id_clone.map(|id| (id, content))
            };

            if let Some((channel_id_clone, content)) = message_data {
                let api_client_clone = state.api_client.clone();

                tokio::spawn(async move {
                    match api_client_clone
                        .create_message(&channel_id_clone, Some(content), false)
                        .await
                    {
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("API Error: {e}");
                        }
                    }
                });
            }
        }
    }
    None
}

async fn move_selection(state: &mut MutexGuard<'_, App>, n: i32, total_filtered_emojis: usize) {
    match state.state {
        AppState::Home => {
            if n < 0 {
                state.selection_index = if state.selection_index == 0 {
                    3 - n.unsigned_abs() as usize
                } else {
                    state.selection_index - n.unsigned_abs() as usize
                };
            } else {
                state.selection_index = (state.selection_index + n.unsigned_abs() as usize) % 3;
            }
        }
        AppState::SelectingDM => {
            if !state.dms.is_empty() {
                if n < 0 {
                    state.selection_index = if state.selection_index == 0 {
                        state.dms.len() - n.unsigned_abs() as usize
                    } else {
                        state.selection_index - n.unsigned_abs() as usize
                    };
                } else {
                    state.selection_index =
                        (state.selection_index + n.unsigned_abs() as usize) % state.dms.len();
                }
            }
        }
        AppState::SelectingGuild => {
            if !state.guilds.is_empty() {
                if n < 0 {
                    state.selection_index = if state.selection_index == 0 {
                        state.guilds.len() - n.unsigned_abs() as usize
                    } else {
                        state.selection_index - n.unsigned_abs() as usize
                    };
                } else {
                    state.selection_index =
                        (state.selection_index + n.unsigned_abs() as usize) % state.guilds.len();
                }
            }
        }
        AppState::SelectingChannel(_) => {
            if !state.channels.is_empty() {
                let filter_text = state.input.to_lowercase();
                let permission_context = &state.context;

                let should_display_content = |c: &Channel| {
                    let is_readable = permission_context
                        .as_ref()
                        .is_some_and(|context| c.is_readable(context));

                    is_readable
                        && (filter_text.is_empty() || c.name.to_lowercase().contains(&filter_text))
                };

                let len: usize = state
                    .channels
                    .iter()
                    .flat_map(|c| {
                        if c.channel_type == 4 {
                            let mut list_items_to_render: Vec<&Channel> = Vec::new();

                            let name_matches = filter_text.is_empty()
                                || c.name.to_lowercase().contains(&filter_text);

                            let child_matches = c.children.as_ref().is_some_and(|children| {
                                children.iter().any(should_display_content)
                            });

                            if name_matches || child_matches {
                                list_items_to_render.push(c);

                                if let Some(children) = &c.children {
                                    list_items_to_render.extend(
                                        children
                                            .iter()
                                            .filter(|child| should_display_content(child)),
                                    );
                                }
                            }
                            list_items_to_render
                        } else if should_display_content(c) {
                            vec![c]
                        } else {
                            vec![]
                        }
                    })
                    .count();

                if n < 0 {
                    state.selection_index = if state.selection_index == 0 {
                        len - n.unsigned_abs() as usize
                    } else {
                        state.selection_index - n.unsigned_abs() as usize
                    };
                } else {
                    state.selection_index =
                        (state.selection_index + n.unsigned_abs() as usize) % len;
                }
            }
        }
        AppState::EmojiSelection(_) => {
            if total_filtered_emojis > 0 {
                if n < 0 {
                    state.selection_index = if state.selection_index == 0 {
                        total_filtered_emojis - 1
                    } else {
                        state.selection_index - 1
                    };
                } else {
                    state.selection_index = (state.selection_index + 1) % total_filtered_emojis;
                }
            }
        }
        _ => {}
    }
}

pub async fn handle_keys_events(
    mut state: MutexGuard<'_, App>,
    action: AppAction,
    tx_action: Sender<AppAction>,
) -> Option<KeywordAction> {
    let state_clone = state.clone();
    let filtered_unicode: Vec<&(String, String)> = state_clone
        .emoji_map
        .iter()
        .filter(|(name, _)| name.starts_with(&state.emoji_filter))
        .collect();

    let state_clone = state.clone();
    let filtered_custom: Vec<&Emoji> = state_clone
        .custom_emojis
        .iter()
        .filter(|e| e.name.starts_with(&state.emoji_filter))
        .collect();

    let total_filtered_emojis = filtered_unicode.len() + filtered_custom.len();

    match action {
        AppAction::SigInt => return Some(KeywordAction::Break),
        AppAction::InputEscape => {
            // In vim mode, Esc switches from Insert to Normal mode and returns early.
            // In non-vim mode (or vim Normal mode), Esc triggers navigation (handled below).
            if state.vim_mode && state.mode == InputMode::Insert {
                state.mode = InputMode::Normal;
                if let Some(c) = state.input[..state.cursor_position].chars().next_back()
                    && c != '\n'
                {
                    state.cursor_position -= c.len_utf8();
                }
                vim::clamp_cursor(&mut state);
                return None;
            }
            // Navigation logic: go back to previous screen or quit
            match &state.state {
                AppState::Home | AppState::Loading(_) => return Some(KeywordAction::Break),
                AppState::SelectingDM => {
                    tx_action.send(AppAction::TransitionToHome).await.ok();
                }
                AppState::SelectingGuild => {
                    tx_action.send(AppAction::TransitionToHome).await.ok();
                }
                AppState::SelectingChannel(_) => {
                    tx_action.send(AppAction::TransitionToGuilds).await.ok();
                }
                AppState::Chatting(channel_id) => {
                    let channel = match state.api_client.get_channel(&channel_id.clone()).await {
                        Ok(c) => c,
                        Err(e) => {
                            tx_action.send(AppAction::TransitionToHome).await.ok();
                            state.status_message = format!("{e}");
                            return None;
                        }
                    };

                    if channel.channel_type == 1 || channel.channel_type == 3 {
                        tx_action.send(AppAction::TransitionToDM).await.ok();
                    } else {
                        match channel.guild_id {
                            Some(guild_id) => tx_action
                                .send(AppAction::TransitionToChannels(guild_id.clone()))
                                .await
                                .ok(),
                            None => tx_action.send(AppAction::TransitionToGuilds).await.ok(),
                        };
                    }
                }
                AppState::EmojiSelection(channel_id) => {
                    tx_action
                        .send(AppAction::TransitionToChat(channel_id.clone()))
                        .await
                        .ok();
                }
            }
        }
        AppAction::Paste(text) => {
            // Always insert text at cursor position, effectively treating it as insert mode operation
            // but without necessarily switching mode if we want to be strict.
            // However, standard behavior usually implies switching to insert or just inserting.
            // Let's just insert.
            let pos = state.cursor_position;
            state.input.insert_str(pos, &text);
            state.cursor_position += text.len();
        }
        AppAction::InputChar(c) => {
            if c == ':' && (!state.vim_mode || state.mode == InputMode::Insert) {
                tx_action.send(AppAction::SelectEmoji).await.ok();
                return None;
            }

            if !state.vim_mode {
                insert_char_at_cursor(&mut state, c);
            } else {
                match state.mode {
                    InputMode::Normal => {
                        vim::handle_vim_keys(state, c, tx_action).await;
                    }
                    InputMode::Insert => {
                        insert_char_at_cursor(&mut state, c);
                    }
                }
            }
        }
        AppAction::SelectEmoji => {
            if let AppState::Chatting(channel_id) = &mut state.clone().state {
                let cursor_pos = std::cmp::min(state.cursor_position, state.input.len());
                let is_start_of_emoji = cursor_pos == 0 || state.input[..cursor_pos].ends_with(' ');

                if is_start_of_emoji {
                    let pos = state.cursor_position;
                    // Track where the emoji filter starts (position of the ':')
                    state.emoji_filter_start = Some(pos);
                    state.input.insert(pos, ':');
                    state.cursor_position += ':'.len_utf8();
                    let owned_channel_id = channel_id.clone();
                    state.state = AppState::EmojiSelection(owned_channel_id);
                    state.status_message =
                        "Type to filter emoji. Enter to select. Esc to cancel.".to_string();
                    state.emoji_filter.clear();
                    state.selection_index = 0;
                } else {
                    let pos = state.cursor_position;
                    state.input.insert(pos, ':');
                    state.cursor_position += ':'.len_utf8();
                }
            }
        }
        AppAction::InputBackspace => {
            if state.vim_mode && state.mode == InputMode::Normal {
                if let Some(c) = state.input[..state.cursor_position].chars().next_back() {
                    state.cursor_position -= c.len_utf8();
                }
                return None;
            }
            let current_state = state.state.clone();
            match current_state {
                AppState::Chatting(_) => {
                    let pos = state.cursor_position;
                    if let Some(c) = state.input[..pos].chars().next_back() {
                        let char_len = c.len_utf8();
                        state.input.remove(pos - char_len);
                        state.cursor_position -= char_len;
                    }
                }
                AppState::EmojiSelection(channel_id) => {
                    let pos = state.cursor_position;
                    if let Some(c) = state.input[..pos].chars().next_back() {
                        let char_len = c.len_utf8();
                        state.input.remove(pos - char_len);
                        state.cursor_position -= char_len;
                        // Recompute emoji_filter based on the current input and emoji_filter_start.
                        if let Some(start) = state.emoji_filter_start {
                            // Position just after the ':' that started the emoji filter.
                            let filter_start = start + ':'.len_utf8();
                            if state.cursor_position <= start || filter_start > state.input.len() {
                                // Cursor moved to or before the ':' (or indices are invalid);
                                // clear the filter as we're no longer within the emoji filter.
                                state.emoji_filter.clear();
                            } else {
                                let end = std::cmp::min(state.cursor_position, state.input.len());
                                if filter_start <= end {
                                    state.emoji_filter = state.input[filter_start..end].to_string();
                                } else {
                                    state.emoji_filter.clear();
                                }
                            }
                        } else {
                            // No known start of emoji filter; be conservative and clear it.
                            state.emoji_filter.clear();
                        }

                        if state.emoji_filter.is_empty() {
                            state.state = AppState::Chatting(channel_id.clone());
                            state.emoji_filter_start = None;
                            state.status_message =
                                "Chatting in channel. Press Enter to send message. Esc to return to channels"
                                    .to_string();
                        }
                        state.selection_index = 0;
                    }
                }
                _ => {
                    let pos = state.cursor_position;
                    if let Some(c) = state.input[..pos].chars().next_back() {
                        let char_len = c.len_utf8();
                        state.input.remove(pos - char_len);
                        state.cursor_position -= char_len;
                    }
                }
            }
        }
        AppAction::InputSubmit => {
            return input_submit(
                &mut state,
                &tx_action,
                filtered_unicode,
                filtered_custom,
                total_filtered_emojis,
            )
            .await;
        }
        AppAction::SelectNext => move_selection(&mut state, 1, total_filtered_emojis).await,
        AppAction::SelectPrevious => move_selection(&mut state, -1, total_filtered_emojis).await,
        AppAction::ApiUpdateMessages(new_messages) => {
            state.messages = new_messages;
        }
        AppAction::ApiUpdateGuilds(new_guilds) => {
            state.guilds = new_guilds.clone();
            state.status_message =
                "Select a server. Use arrows to navigate, Enter to select & Esc to quit."
                    .to_string();
        }
        AppAction::ApiUpdateChannel(new_channels) => {
            state.channels =
                Channel::filter_channels_by_categories(new_channels).unwrap_or_default();
            let text_channels_count = state.channels.len();
            if text_channels_count > 0 {
                state.status_message =
                    "Channels loaded. Select one to chat. (Esc to return to Servers)".to_string();
            } else {
                state.status_message =
                    "No text channels found. (Esc to return to Servers)".to_string();
            }
            state.selection_index = 0;
        }
        AppAction::ApiUpdateEmojis(new_emojis) => {
            state.custom_emojis = new_emojis;
        }
        AppAction::ApiUpdateDMs(new_dms) => {
            state.dms = new_dms;
            let dms_count = state.dms.len();
            if dms_count > 0 {
                state.status_message =
                    "DMs loaded. Select one to chat. (Esc to return to Home)".to_string();
            } else {
                state.status_message = "No DMs found. (Esc to return to Home)".to_string();
            }
            state.selection_index = 0;
        }
        AppAction::ApiUpdateContext(new_context) => {
            state.context = new_context;
        }
        AppAction::TransitionToChannels(guild_id) => {
            state.input = String::new();
            state.cursor_position = 0;
            state.state = AppState::SelectingChannel(guild_id);
            state.status_message =
                "Select a server. Use arrows to navigate, Enter to select & Esc to quit"
                    .to_string();
            state.selection_index = 0;
        }
        AppAction::TransitionToChat(channel_id) => {
            // Check if we're coming from emoji selection before changing state
            if let AppState::EmojiSelection(_) = &state.state {
                // Remove the trailing ':' and filter text if canceling emoji selection
                if let Some(start) = state.emoji_filter_start {
                    let end = start + ':'.len_utf8() + state.emoji_filter.len();
                    if state.input.is_char_boundary(start) && state.input.is_char_boundary(end) {
                        state.input.drain(start..end);
                        state.cursor_position = start;
                    }
                }
                state.emoji_filter.clear();
                state.emoji_filter_start = None;
                state.selection_index = 0;
            }
            state.state = AppState::Chatting(channel_id.clone());
            state.status_message =
                "Chatting in channel. Press Enter to send message, Esc to return to channels."
                    .to_string();
        }
        AppAction::TransitionToGuilds => {
            state.input = String::new();
            state.cursor_position = 0;
            state.state = AppState::SelectingGuild;
            state.status_message =
                "Select a server. Use arrows to navigate, Enter to select & Esc to quit"
                    .to_string();
            state.selection_index = 0;
        }
        AppAction::TransitionToDM => {
            state.input = String::new();
            state.cursor_position = 0;
            state.state = AppState::SelectingDM;
            state.status_message =
                "Select a DM. Use arrows to navigate, Enter to select & Esc to quit".to_string();
            state.selection_index = 0;
        }
        AppAction::TransitionToHome => {
            state.input = String::new();
            state.cursor_position = 0;
            state.state = AppState::Home;
            state.status_message = "Browse either DMs or Servers. Use arrows to navigate, Enter to select & Esc to quit".to_string();
            state.selection_index = 0;
        }
        AppAction::TransitionToLoading(redirect_state) => {
            state.state = AppState::Loading(redirect_state);
            state.status_message = "Loading...".to_string();
        }
        AppAction::EndLoading => {
            if let AppState::Loading(redirect) = &state.clone().state {
                match redirect {
                    Window::Home => tx_action.send(AppAction::TransitionToHome).await.ok(),
                    Window::Guild => tx_action.send(AppAction::TransitionToGuilds).await.ok(),
                    Window::DM => tx_action.send(AppAction::TransitionToDM).await.ok(),
                    Window::Channel(guild_id) => tx_action
                        .send(AppAction::TransitionToChannels(guild_id.clone()))
                        .await
                        .ok(),
                    Window::Chat(channel_id) => tx_action
                        .send(AppAction::TransitionToChat(channel_id.clone()))
                        .await
                        .ok(),
                };
            }
        }
        AppAction::Tick => {
            state.tick_count = state.tick_count.wrapping_add(1);
            return Some(KeywordAction::Continue);
        }
    }

    None
}
