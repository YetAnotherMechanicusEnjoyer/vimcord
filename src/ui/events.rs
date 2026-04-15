use std::io;

use crossterm::event::{self, KeyCode, KeyEventKind};
use tokio::{
    sync::{MutexGuard, mpsc::Sender},
    time::{self, Duration},
};

use crate::{
    App, AppAction, AppState, InputMode, KeywordAction, Window,
    api::{AnyChannel, Channel, DM, Emoji, Message, guild::PartialGuild},
    logs::{LogType, print_log},
    ui::vim,
};

/// Helper function to insert a character at the cursor position.
/// Handles both emoji selection state and normal input state.
fn insert_char_at_cursor(state: &mut MutexGuard<'_, App>, tx_action: Sender<AppAction>, c: char) {
    let current_state = state.state.clone();
    match current_state {
        AppState::EmojiSelection(channel) => {
            let pos = state.cursor_position;
            state.input.insert(pos, c);
            state.cursor_position += c.len_utf8();
            if c == ' ' {
                #[allow(unused)]
                tx_action.send(AppAction::TransitionToChat(channel));
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
                    #[allow(unused)]
                    tx_action.send(AppAction::TransitionToChat(channel));
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
                                        KeyCode::Delete => {
                                            tx.send(AppAction::InputDelete).await.ok();
                                        }
                                        KeyCode::Up => {
                                            tx.send(AppAction::SelectPrevious).await.ok();
                                        }
                                        KeyCode::Down => {
                                            tx.send(AppAction::SelectNext).await.ok();
                                        }
                                        KeyCode::Left => {
                                            tx.send(AppAction::SelectLeft).await.ok();
                                        }
                                        KeyCode::Right => {
                                            tx.send(AppAction::SelectRight).await.ok();
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
    if state.vim_mode && (state.mode == InputMode::Command || state.mode == InputMode::Search) {
        let input = state.input.clone();

        state.input = state.saved_input.clone().unwrap_or_default();
        state.saved_input = None;
        let pos = if state.cursor_position <= state.input.len() && state.cursor_position > 0 {
            state.cursor_position
        } else {
            state.input.len()
        };
        if let Some(c) = state.input[..pos].chars().next_back()
            && c != '\n'
        {
            state.cursor_position = state.cursor_position.saturating_sub(c.len_utf8());
        }
        if !(state.cursor_position == state.input.len() && state.input.ends_with('\n')) {
            vim::clamp_cursor(state);
        }

        match &state.mode {
            InputMode::Command => {
                if let Some(action) = crate::ui::commands::handle_command(state, tx_action, input).await {
                    return Some(action);
                }
            }
            InputMode::Search => {
                state.search_input = input;
            }
            _ => {}
        }
        state.mode = InputMode::Normal;
        return None;
    }
    match state.state.clone() {
        AppState::Loading(_) | AppState::Logs(_) => {}
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
            let filter_text = state.search_input.to_lowercase();
            let dms: Vec<&DM> = state
                .dms
                .iter()
                .filter(|d| d.get_name().to_lowercase().contains(&filter_text))
                .collect();

            if dms.is_empty() {
                return Some(KeywordAction::Continue);
            }

            let selected_dm = dms[state.selection_index].clone();
            let selected_dm_name = if selected_dm.recipients.is_empty() {
                "Empty".to_string()
            } else {
                selected_dm.recipients[0].username.clone()
            };

            state.cursor_position = 0;
            state.status_message = format!("Loading messages for {selected_dm_name}...");

            let tx_action_clone = tx_action.clone();
            let api_client_clone = state.api_client.clone();
            let channel_load = selected_dm.clone();

            tokio::spawn(async move {
                tx_action_clone
                    .send(AppAction::TransitionToLoading(Window::Chat(Box::new(
                        AnyChannel::Direct(channel_load.clone()),
                    ))))
                    .await
                    .ok();

                match api_client_clone
                    .get_channel_messages(&channel_load.id, None, None, None, Some(100))
                    .await
                {
                    Ok(messages) => {
                        if let Err(e) = tx_action_clone
                            .send(AppAction::ApiUpdateMessages(
                                channel_load.id.clone(),
                                messages,
                            ))
                            .await
                        {
                            print_log(
                                format!("Failed to send message update action: {e}").into(),
                                LogType::Error,
                            )
                            .await
                            .ok();
                        }
                    }
                    Err(e) => {
                        print_log(format!("Error loading DM chat: {e}").into(), LogType::Error)
                            .await
                            .ok();
                    }
                }

                tx_action_clone.send(AppAction::EndLoading).await.ok();
            });
        }
        AppState::SelectingGuild => {
            let filter_text = state.search_input.to_lowercase();
            let guilds: Vec<&PartialGuild> = state
                .guilds
                .iter()
                .filter(|g| g.name.to_lowercase().contains(&filter_text))
                .collect();

            if guilds.is_empty() {
                return Some(KeywordAction::Continue);
            }

            let selected_partial_guild = guilds[state.selection_index].clone();
            let selected_guild_name = selected_partial_guild.name.clone();

            let selected_guild = match state
                .api_client
                .get_guild(selected_partial_guild.id.as_str())
                .await
            {
                Ok(g) => g,
                Err(e) => {
                    print_log(e, LogType::Error).await.ok();
                    tx_action.send(AppAction::TransitionToGuilds).await.ok();
                    return None;
                }
            };

            if let Err(e) = state
                .gateway_client
                .request_guild_members(selected_partial_guild.id.as_str())
                .await
            {
                print_log(
                    format!("Error requesting guild members: {e}").into(),
                    LogType::Error,
                )
                .await
                .ok();
            }

            state.selected_guild = Some(selected_guild.clone());

            let tx_clone = tx_action.clone();

            state.status_message = format!("Loading channels for {selected_guild_name}...");

            let api_client_clone = state.api_client.clone();

            tokio::spawn(async move {
                tx_clone
                    .send(AppAction::TransitionToLoading(Window::Channel(Box::new(
                        selected_guild.clone(),
                    ))))
                    .await
                    .ok();
                match api_client_clone
                    .get_guild_channels(selected_guild.id.as_str())
                    .await
                {
                    Ok(channels) => {
                        tx_clone
                            .send(AppAction::ApiUpdateChannel(channels))
                            .await
                            .ok();
                    }
                    Err(e) => {
                        print_log(
                            format!("Failed to load channels: {e}").into(),
                            LogType::Error,
                        )
                        .await
                        .ok();
                    }
                }
                match api_client_clone
                    .get_guild_emojis(selected_guild.id.as_str())
                    .await
                {
                    Ok(emojis) => {
                        tx_clone.send(AppAction::ApiUpdateEmojis(emojis)).await.ok();
                    }
                    Err(e) => {
                        print_log(
                            format!("Failed to load custom emojis: {e}").into(),
                            LogType::Error,
                        )
                        .await
                        .ok();
                    }
                }
                match api_client_clone
                    .get_permission_context(selected_guild.id.as_str())
                    .await
                {
                    Ok(context) => {
                        tx_clone
                            .send(AppAction::ApiUpdateContext(Some(context)))
                            .await
                            .ok();
                    }
                    Err(e) => {
                        print_log(
                            format!("Failed to load permission context: {e}").into(),
                            LogType::Error,
                        )
                        .await
                        .ok();
                    }
                }

                tx_clone.send(AppAction::EndLoading).await.ok();
            });
        }
        AppState::SelectingChannel(g) => {
            let permission_context = &state.context;
            let mut text_channels: Vec<&Channel> = Vec::new();

            let filter_text = state.search_input.to_lowercase();
            let should_display_channel_content = |c: &Channel| {
                let is_readable = permission_context
                    .as_ref()
                    .is_some_and(|context| c.is_readable(context));

                is_readable
                    && (filter_text.is_empty() || c.name.to_lowercase().contains(&filter_text))
            };

            state
                .channels
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
                    if let Some(children) = &c.children {
                        text_channels.push(c);

                        children
                            .iter()
                            .filter(|c| should_display_channel_content(c))
                            .for_each(|c| text_channels.push(c));
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

            let selected_channel = text_channels[state.selection_index].clone();

            tx_action
                .send(AppAction::TransitionToLoading(Window::Chat(Box::new(
                    AnyChannel::Guild(selected_channel.clone()),
                ))))
                .await
                .ok();

            if let Err(e) = state
                .gateway_client
                .subscribe_channel(&g.id, &selected_channel.id)
                .await
            {
                print_log(
                    format!("Failed to subscribe to a channel: {e}").into(),
                    LogType::Error,
                )
                .await
                .ok();
            }

            state.input = String::new();
            state.cursor_position = 0;
            state.status_message = format!("Loading messages for {}...", selected_channel.name);

            match state
                .api_client
                .get_channel_messages(
                    selected_channel.id.clone().as_str(),
                    None,
                    None,
                    None,
                    Some(100),
                )
                .await
            {
                Ok(messages) => {
                    if let Err(e) = tx_action
                        .send(AppAction::ApiUpdateMessages(
                            selected_channel.id.clone(),
                            messages,
                        ))
                        .await
                    {
                        print_log(
                            format!("Failed to send message update action: {e}").into(),
                            LogType::Error,
                        )
                        .await
                        .ok();
                        return None;
                    }
                }
                Err(e) => {
                    state.status_message = format!("Error loading chat: {e}");
                }
            }

            tx_action.send(AppAction::EndLoading).await.ok();
        }
        AppState::EmojiSelection(channel) => {
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

            state.state = AppState::Chatting(channel.clone());
            state.emoji_filter.clear();
            state.emoji_filter_start = None;
            state.emoji_index = 0;
            state.status_message =
                "Chatting in channel. Press Enter to send message, Esc to return to channels."
                    .to_string();
        }
        AppState::Editing(channel, message, _) => {
            let (channel_id_clone, message_id_clone) =
                (channel.get_id().clone(), message.id.clone());
            let content = state.input.drain(..).collect::<String>();

            let message_data = if content.is_empty() {
                None
            } else {
                Some((channel_id_clone, content))
            };

            let tx_action_clone = tx_action.clone();

            if let Some((channel_id_clone, content)) = message_data {
                let api_client_clone = state.api_client.clone();
                let msgs = state.messages.clone();

                tokio::spawn(async move {
                    match api_client_clone
                        .edit_message(&channel_id_clone, &message_id_clone, Some(content))
                        .await
                    {
                        Ok(msg) => {
                            let _ = tx_action_clone
                                .send(AppAction::ApiUpdateMessages(
                                    channel_id_clone.clone(),
                                    msgs.iter()
                                        .map(|m| {
                                            if m.id == msg.id {
                                                msg.clone()
                                            } else {
                                                m.clone()
                                            }
                                        })
                                        .collect::<Vec<Message>>(),
                                ))
                                .await;
                            let _ = tx_action_clone
                                .send(AppAction::TransitionToChat(channel.clone()))
                                .await
                                .ok();
                        }
                        Err(e) => {
                            print_log(format!("API Error: {e}").into(), LogType::Error)
                                .await
                                .ok();
                        }
                    }
                });
            }
        }
        AppState::Chatting(channel) => {
            let channel_id_clone = channel.get_id().clone();

            let content = state.input.drain(..).collect::<String>();
            state.cursor_position = 0;

            let message_data = if content.is_empty() || channel_id_clone.is_empty() {
                None
            } else {
                Some((channel_id_clone, content))
            };

            if let Some((channel_id_clone, content)) = message_data {
                let api_client_clone = state.api_client.clone();

                let mut members = std::collections::HashSet::new();
                let tmp = content.clone();
                let mut temp_content = tmp.as_str();

                while let Some(start_idx) = temp_content.find("@") {
                    let after_prefix = &temp_content[start_idx + 1..];
                    if let Some(end_idx) = after_prefix.find(' ') {
                        let member = &after_prefix[..end_idx];
                        if member.chars().all(|c| c.is_ascii_alphanumeric()) {
                            members.insert(member.to_string());
                        }
                        temp_content = &after_prefix[end_idx..];
                    } else {
                        let end_idx = temp_content.len().saturating_sub(1);
                        let member = &after_prefix[..end_idx];
                        if member.chars().all(|c| c.is_ascii_alphanumeric()) {
                            members.insert(member.to_string());
                        }
                        break;
                    }
                }

                let mut map_usernames: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                for member in members {
                    if let Some(guild_member) = state
                        .guild_members
                        .iter()
                        .find(|gm| gm.user.username == member)
                    {
                        map_usernames.insert(member.clone(), guild_member.user.id.clone());
                    }
                }

                let mut final_content = content.clone();
                for (name, id) in map_usernames {
                    let pattern = format!("@{name}");
                    let replacement = format!("<@{id}>");
                    final_content = final_content.replace(&pattern, &replacement);
                }

                tokio::spawn(async move {
                    match api_client_clone
                        .create_message(&channel_id_clone, Some(final_content), false)
                        .await
                    {
                        Ok(_) => {}
                        Err(e) => {
                            print_log(format!("API Error: {e}").into(), LogType::Error)
                                .await
                                .ok();
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
                let filter_text = state.search_input.to_lowercase();
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
                    state.emoji_index = if state.emoji_index == 0 {
                        total_filtered_emojis - 1
                    } else {
                        state.emoji_index - 1
                    };
                } else {
                    state.emoji_index = (state.emoji_index + 1) % total_filtered_emojis;
                }
            }
        }
        _ => {}
    }
}

fn handle_user_typing(state: &mut App) {
    if state.silent_typing {
        return;
    }
    if let AppState::Chatting(channel) = &state.state {
        let now = std::time::Instant::now();
        let should_send = state
            .last_typing_sent
            .is_none_or(|last| now.duration_since(last).as_secs() >= 8);
        if should_send && !state.input.is_empty() {
            state.last_typing_sent = Some(now);
            let api_client_clone = state.api_client.clone();
            let channel_id_clone = channel.get_id().clone();
            tokio::spawn(async move {
                let _ = api_client_clone
                    .trigger_typing_indicator(&channel_id_clone)
                    .await;
            });
        }
    }
}

pub async fn handle_keys_events(
    mut state: MutexGuard<'_, App>,
    action: AppAction,
    tx_action: Sender<AppAction>,
) -> Option<KeywordAction> {
    let emoji_map_clone = state.emoji_map.clone();
    let filtered_unicode: Vec<&(String, String)> = emoji_map_clone
        .iter()
        .filter(|(name, _)| name.starts_with(&state.emoji_filter))
        .collect();

    let custom_emojis_clone = state.custom_emojis.clone();
    let filtered_custom: Vec<&Emoji> = custom_emojis_clone
        .iter()
        .filter(|e| e.name.starts_with(&state.emoji_filter))
        .collect();

    let total_filtered_emojis = filtered_unicode.len() + filtered_custom.len();

    match action {
        AppAction::SigInt => return Some(KeywordAction::Break),
        AppAction::InputEscape => {
            // In vim mode, Esc switches from Insert to Normal mode and returns early.
            // In non-vim mode (or vim Normal mode), Esc triggers navigation (handled below).
            if state.vim_mode && state.mode == InputMode::Insert
                || state.mode == InputMode::Command
                || state.mode == InputMode::Search
            {
                state.mode = InputMode::Normal;
                if state.mode == InputMode::Command || state.mode == InputMode::Search {
                    state.input = state.saved_input.clone().unwrap_or_default();
                    state.saved_input = None;
                }
                let pos = if state.cursor_position <= state.input.len() && state.cursor_position > 0
                {
                    state.cursor_position
                } else {
                    state.input.len()
                };
                if let Some(c) = state.input[..pos].chars().next_back()
                    && c != '\n'
                {
                    state.cursor_position = state.cursor_position.saturating_sub(c.len_utf8());
                }
                if !(state.cursor_position == state.input.len() && state.input.ends_with('\n')) {
                    vim::clamp_cursor(&mut state);
                }
                return None;
            }
            // Navigation logic: go back to previous screen or quit
            match &state.state {
                AppState::Home | AppState::Loading(_) => {}
                AppState::SelectingDM => {
                    tx_action.send(AppAction::TransitionToHome).await.ok();
                }
                AppState::SelectingGuild => {
                    tx_action.send(AppAction::TransitionToHome).await.ok();
                }
                AppState::SelectingChannel(_) => {
                    state.guild_members = Vec::new();
                    state.selected_guild = None;
                    tx_action.send(AppAction::TransitionToGuilds).await.ok();
                }
                AppState::Chatting(channel) => {
                    let channel = match state
                        .api_client
                        .get_channel(channel.get_id().as_str())
                        .await
                    {
                        Ok(c) => match c {
                            AnyChannel::Guild(ch) => ch,
                            AnyChannel::Direct(_) => {
                                tx_action.send(AppAction::TransitionToDM).await.ok();
                                return None;
                            }
                        },
                        Err(e) => {
                            print_log(e, LogType::Error).await.ok();
                            return None;
                        }
                    };

                    if channel.channel_type == 1 || channel.channel_type == 3 {
                        tx_action.send(AppAction::TransitionToDM).await.ok();
                    } else {
                        match channel.guild_id {
                            Some(guild_id) => {
                                let guild = state.api_client.get_guild(guild_id.as_str()).await;
                                if let Ok(guild) = guild {
                                    tx_action
                                        .send(AppAction::TransitionToChannels(Box::new(
                                            guild.clone(),
                                        )))
                                        .await
                                        .ok()
                                } else {
                                    tx_action.send(AppAction::TransitionToGuilds).await.ok()
                                }
                            }
                            None => tx_action.send(AppAction::TransitionToGuilds).await.ok(),
                        };
                    }
                }
                AppState::EmojiSelection(channel) => {
                    tx_action
                        .send(AppAction::TransitionToChat(channel.clone()))
                        .await
                        .ok();
                }
                AppState::Editing(channel, _, _) => {
                    tx_action
                        .send(AppAction::TransitionToChat(channel.clone()))
                        .await
                        .ok();
                }
                AppState::Logs(redirect) => {
                    match redirect {
                        Window::Home => tx_action.send(AppAction::TransitionToHome).await.ok(),
                        Window::Guild => tx_action.send(AppAction::TransitionToGuilds).await.ok(),
                        Window::DM => tx_action.send(AppAction::TransitionToDM).await.ok(),
                        Window::Channel(guild) => tx_action
                            .send(AppAction::TransitionToChannels(guild.clone()))
                            .await
                            .ok(),
                        Window::Chat(channel) => tx_action
                            .send(AppAction::TransitionToChat(channel.clone()))
                            .await
                            .ok(),
                    };
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
            handle_user_typing(&mut state);
        }
        AppAction::InputChar(c) => {
            if c == ':' && (!state.vim_mode || state.mode == InputMode::Insert) {
                tx_action.send(AppAction::SelectEmoji).await.ok();
                return None;
            }

            if !state.vim_mode {
                insert_char_at_cursor(&mut state, tx_action.clone(), c);
                handle_user_typing(&mut state);
            } else {
                match state.mode {
                    InputMode::Normal => {
                        vim::handle_vim_keys(state, c, tx_action).await;
                    }
                    InputMode::Insert | InputMode::Command | InputMode::Search => {
                        insert_char_at_cursor(&mut state, tx_action.clone(), c);
                        handle_user_typing(&mut state);
                    }
                }
            }
        }
        AppAction::SelectEmoji => {
            if let AppState::Chatting(channel) | AppState::Editing(channel, _, _) =
                state.state.clone()
            {
                let cursor_pos = std::cmp::min(state.cursor_position, state.input.len());
                let is_start_of_emoji = cursor_pos == 0 || state.input[..cursor_pos].ends_with(' ');

                if is_start_of_emoji {
                    let pos = state.cursor_position;
                    // Track where the emoji filter starts (position of the ':')
                    state.emoji_filter_start = Some(pos);
                    state.input.insert(pos, ':');
                    state.cursor_position += ':'.len_utf8();
                    let owned_channel = channel.clone();
                    state.state = AppState::EmojiSelection(owned_channel);
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
            if state.vim_mode
                && (state.mode == InputMode::Command || state.mode == InputMode::Search)
                && state.input.is_empty()
            {
                state.mode = InputMode::Normal;
                state.input = state.saved_input.clone().unwrap_or_default();
                state.saved_input = None;
                let pos = if state.cursor_position <= state.input.len() && state.cursor_position > 0
                {
                    state.cursor_position
                } else {
                    state.input.len()
                };
                if let Some(c) = state.input[..pos].chars().next_back()
                    && c != '\n'
                {
                    state.cursor_position = state.cursor_position.saturating_sub(c.len_utf8());
                }
                if !(state.cursor_position == state.input.len() && state.input.ends_with('\n')) {
                    vim::clamp_cursor(&mut state);
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
                    handle_user_typing(&mut state);
                }
                AppState::EmojiSelection(channel) => {
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
                            state.state = AppState::Chatting(channel.clone());
                            state.emoji_filter_start = None;
                            state.status_message =
                                "Chatting in channel. Press Enter to send message. Esc to return to channels"
                                    .to_string();
                        }
                        state.selection_index = 0;
                    }
                }
                _ => {
                    let pos = if state.cursor_position <= state.input.len()
                        && state.cursor_position > 0
                    {
                        state.cursor_position
                    } else {
                        state.input.len()
                    };
                    if let Some(c) = state.input[..pos].chars().next_back() {
                        let char_len = c.len_utf8();
                        state.input.remove(pos - char_len);
                        state.cursor_position -= char_len;
                    }
                }
            }
        }
        AppAction::InputDelete => {
            let current_state = state.state.clone();
            match current_state {
                AppState::Chatting(_) => {
                    if !state.input.is_empty() {
                        let pos = {
                            if state.cursor_position >= state.input.len() {
                                state.cursor_position = state.input.len().saturating_sub(1);
                            }
                            state.cursor_position.saturating_add(1)
                        };
                        if let Some(c) = state.input[..pos].chars().next_back() {
                            let char_len = c.len_utf8();
                            state.input.remove(pos - char_len);
                        }
                    }
                    handle_user_typing(&mut state);
                }
                AppState::EmojiSelection(channel) => {
                    let pos = state.cursor_position + 1;
                    if let Some(c) = state.input[..pos].chars().next_back() {
                        let char_len = c.len_utf8();
                        state.input.remove(pos - char_len);
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
                            state.state = AppState::Chatting(channel.clone());
                            state.emoji_filter_start = None;
                            state.status_message =
                                "Chatting in channel. Press Enter to send message. Esc to return to channels"
                                    .to_string();
                        }
                        state.emoji_index = 0;
                    }
                }
                _ => {
                    let pos = if state.cursor_position < state.input.len() {
                        state.cursor_position + 1
                    } else {
                        state.input.len()
                    };
                    if let Some(c) = state.input[..pos].chars().next_back() {
                        let char_len = c.len_utf8();
                        state.input.remove(pos - char_len);
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
        AppAction::SelectLeft => {
            vim::handle_vim_keys(state, 'h', tx_action).await;
        }
        AppAction::SelectRight => {
            vim::handle_vim_keys(state, 'l', tx_action).await;
        }
        AppAction::ApiUpdateMessages(channel_id, new_messages) => {
            let is_relevant = match &state.state {
                AppState::Loading(Window::Chat(channel)) => channel.get_id() == channel_id,
                AppState::Chatting(channel) => channel.get_id() == channel_id,
                AppState::Editing(channel, _, _) => channel.get_id() == channel_id,
                AppState::EmojiSelection(channel) => channel.get_id() == channel_id,
                _ => false,
            };
            if !is_relevant {
                return None;
            }

            if let Some(newest_msg) = new_messages.iter().max_by_key(|m| &m.id) {
                // Check if the newest message is newer than what we currently have
                let should_ack = if let Some(last_id) = state.last_message_ids.get(&channel_id) {
                    // Message IDs can be compared as u64 safely
                    let new_id_num = newest_msg.id.parse::<u64>().unwrap_or_default();
                    let last_id_num = last_id.parse::<u64>().unwrap_or_default();
                    new_id_num > last_id_num
                } else {
                    true
                };

                if should_ack {
                    state
                        .last_message_ids
                        .insert(channel_id.clone(), newest_msg.id.clone());

                    let api_client_clone = state.api_client.clone();
                    let channel_id_clone = channel_id.clone();
                    let msg_id_clone = newest_msg.id.clone();
                    tokio::spawn(async move {
                        if let Err(e) = api_client_clone
                            .ack_message(&channel_id_clone, &msg_id_clone)
                            .await
                        {
                            print_log(format!("Failed to ack message: {e}").into(), LogType::Error)
                                .await
                                .ok();
                        }
                    });
                }
            }

            // Clear any active desktop notifications for this channel
            #[cfg(all(unix, not(target_os = "macos")))]
            if let Some(handles) = state.active_notifications.remove(&channel_id) {
                for handle in handles {
                    handle.close();
                }
            }
            // Seed the username cache from all loaded message authors
            for msg in &new_messages {
                state
                    .user_names
                    .insert(msg.author.id.clone(), msg.author.username.clone());
            }
            state.messages = new_messages
                .into_iter()
                .filter(|m| !state.deleted_message_ids.contains(&m.id))
                .collect();
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
        AppAction::ApiUpdateDMs(mut new_dms) => {
            // Sort DMs by newest last_message_id
            new_dms.sort_by_key(|dm| {
                std::cmp::Reverse(
                    dm.last_message_id
                        .as_deref()
                        .unwrap_or("0")
                        .parse::<u64>()
                        .unwrap_or(0),
                )
            });
            state.dms = new_dms.clone();

            // Initialize last_message_ids for all DMs on load and seed username cache
            for dm in new_dms {
                // Seed user_names from all DM recipients
                for recipient in &dm.recipients {
                    state
                        .user_names
                        .insert(recipient.id.clone(), recipient.username.clone());
                }
                if let Some(msg_id) = dm.last_message_id {
                    // Only insert if it doesn't already exist so we don't accidentally
                    // overwrite during a mid-session refresh
                    state.last_message_ids.entry(dm.id).or_insert(msg_id);
                }
            }

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
        AppAction::ApiUpdateCurrentUser(user) => {
            state.current_user = Some(user);
        }
        AppAction::GatewayTypingStart(channel_id, user_id, display_name) => {
            // Typing indicator expires after 10 seconds or when the user sends a message
            let now = std::time::Instant::now();
            let channel_typers = state.typing_users.entry(channel_id).or_default();
            channel_typers.insert(user_id.clone(), now);
            // Cache the display name if provided by the gateway event
            if let Some(name) = display_name {
                state.user_names.insert(user_id, name);
            }
        }
        AppAction::GatewayMessageCreate(msg) => {
            let active_channel_id = if let AppState::Chatting(channel) = &state.state {
                Some(channel.get_id().clone())
            } else {
                None
            };

            if Some(msg.channel_id.clone()) == active_channel_id {
                let mut msgs = state.messages.clone();
                // Cache author username from incoming message
                state
                    .user_names
                    .insert(msg.author.id.clone(), msg.author.username.clone());
                msgs.push(msg.clone());
                // Sort by descending ID: newest messages first (to match REST API response)
                msgs.sort_by_key(|m| std::cmp::Reverse(m.id.parse::<u64>().unwrap_or_default()));
                if state.selection_index > 0
                    && let AppState::Chatting(_) = &state.state
                {
                    state.selection_index += 1;
                }
                state.messages = msgs;

                state
                    .last_message_ids
                    .insert(msg.channel_id.clone(), msg.id.clone());

                let api_client_clone = state.api_client.clone();
                let channel_id_clone = msg.channel_id.clone();
                let msg_id_clone = msg.id.clone();
                tokio::spawn(async move {
                    let _ = api_client_clone
                        .ack_message(&channel_id_clone, &msg_id_clone)
                        .await;
                });
            } else {
                let is_dm = state.dms.iter().any(|dm| dm.id == msg.channel_id);
                let is_mentioned = state
                    .current_user
                    .as_ref()
                    .is_some_and(|u| msg.mentions.iter().any(|m| m.id == u.id));

                if is_dm || is_mentioned {
                    let is_self = state
                        .current_user
                        .as_ref()
                        .is_some_and(|u| u.id == msg.author.id);

                    if !is_self {
                        let sender = if state.notifs_display_username {
                            msg.author.username.clone()
                        } else {
                            msg.author
                                .global_name
                                .clone()
                                .unwrap_or_else(|| msg.author.username.clone())
                        };
                        let is_dm_clone = is_dm;
                        let msg_clone = msg.clone();
                        let discreet = state.discreet_notifs;

                        let guild_clone = state.selected_guild.as_ref().and_then(|sg| {
                            if msg.guild_id.as_deref() == Some(sg.id.as_str()) {
                                Some(sg.clone())
                            } else {
                                None
                            }
                        });

                        let guild_name = msg.guild_id.as_ref().and_then(|gid| {
                            state
                                .guilds
                                .iter()
                                .find(|g| &g.id == gid)
                                .map(|g| g.name.clone())
                        });

                        let mut cached_channel_name = None;
                        if msg.guild_id.is_some()
                            && state.selected_guild.as_ref().map(|g| &g.id) == msg.guild_id.as_ref()
                        {
                            for channel in &state.channels {
                                if channel.id == msg.channel_id {
                                    cached_channel_name = Some(channel.name.clone());
                                    break;
                                }
                                if let Some(children) = &channel.children
                                    && let Some(child) =
                                        children.iter().find(|c| c.id == msg.channel_id)
                                {
                                    cached_channel_name = Some(child.name.clone());
                                    break;
                                }
                            }
                        }

                        let api_client = state.api_client.clone();
                        let tx = tx_action.clone();

                        let is_dnd = state.is_invisible_dnd
                            || state
                                .current_user
                                .as_ref()
                                .and_then(|u| state.user_statuses.get(&u.id))
                                .map(|s| s.as_str())
                                == Some("dnd");

                        if !is_dnd {
                            tokio::spawn(async move {
                                let (summary, body) = if discreet {
                                    let body = if is_dm_clone {
                                        "Sent you a DM".to_string()
                                    } else {
                                        "Mentioned you in a channel".to_string()
                                    };
                                    (sender, body)
                                } else {
                                    let body =
                                        if msg_clone.content.as_ref().is_some_and(|c| !c.is_empty()) {
                                            msg_clone.map_mentions(guild_clone)
                                        } else {
                                            "Sent an attachment".to_string()
                                        };
                                    let mut final_sender = sender.clone();

                                    if !is_dm_clone {
                                        let mut channel_name = String::new();
                                        if let Some(name) = cached_channel_name {
                                            channel_name = format!("#{}", name);
                                        } else if let Ok(crate::api::AnyChannel::Guild(c)) =
                                            api_client.get_channel(&msg_clone.channel_id).await
                                        {
                                            channel_name = format!("#{}", c.name);
                                        }

                                        if let Some(gn) = guild_name {
                                            if !channel_name.is_empty() {
                                                final_sender =
                                                    format!("{} in {} ({})", sender, gn, channel_name);
                                            } else {
                                                final_sender = format!("{} in {}", sender, gn);
                                            }
                                        } else if !channel_name.is_empty() {
                                            final_sender = format!("{} in {}", sender, channel_name);
                                        }
                                    }

                                    (final_sender, body)
                                };

                                let _ = tx
                                    .send(AppAction::DesktopNotification(
                                        summary,
                                        body,
                                        msg_clone.channel_id,
                                    ))
                                    .await;
                            });
                        }
                    }

                    if is_dm {
                        // Jump this DM to the top of the list
                        if let Some(pos) = state.dms.iter().position(|dm| dm.id == msg.channel_id) {
                            let mut dm = state.dms.remove(pos);
                            dm.last_message_id = Some(msg.id.clone());
                            state.dms.insert(0, dm);
                        }

                        state
                            .last_message_ids
                            .insert(msg.channel_id.clone(), msg.id.clone());
                    }
                }
            }

            // Remove the typing indicator if the author sent a message in the channel
            if let Some(typers) = state.typing_users.get_mut(&msg.channel_id) {
                typers.remove(&msg.author.id);
            }
        }
        AppAction::GatewayReadySupplemental(statuses, status_texts) => {
            state.user_statuses.extend(statuses);
            state.user_status_texts.extend(status_texts);
        }
        AppAction::GatewayPresenceUpdate(presence) => {
            state
                .user_statuses
                .insert(presence.user.id.clone(), presence.status);
            if let Some(text) = presence.activities.iter().find_map(|a| {
                if a.activity_type == 4 {
                    Some(a.state.clone())
                } else {
                    None
                }
            }) {
                state
                    .user_status_texts
                    .insert(presence.user.id, text.unwrap_or_default());
            } else {
                state.user_status_texts.remove(&presence.user.id);
            }
        }
        AppAction::GatewayGuildMembersChunk(_, members, _, chunk_count) => {
            if chunk_count.parse::<usize>().unwrap_or_default() == 0 {
                state.guild_members = Vec::new();
            }
            state.guild_members.extend(members);
        }
        AppAction::GatewayMessageUpdate(msg) => {
            let mut msgs = state.messages.clone();
            if let Some(pos) = msgs.iter().position(|m| m.id == msg.id) {
                let mut existing = msgs[pos].clone();
                if let Some(content) = msg.content {
                    existing.content = Some(content);
                }
                if let Some(author) = msg.author {
                    existing.author = author;
                }
                if let Some(timestamp) = msg.timestamp {
                    existing.timestamp = timestamp;
                }
                msgs[pos] = existing;
                state.messages = msgs;
            }
        }
        AppAction::GatewayMessageDelete(id, _channel_id) => {
            let mut msgs = state.messages.clone();
            msgs.retain(|m| m.id != id);
            state.messages = msgs;
            state.deleted_message_ids.insert(id);
            if state.selection_index > 0
                && let AppState::Chatting(_) = &state.state
            {
                state.selection_index -= 1;
            }
        }
        AppAction::TransitionToChannels(guild) => {
            state.input = String::new();
            state.search_input = String::new();
            state.cursor_position = 0;
            state.state = AppState::SelectingChannel(guild.clone());
            state.status_message =
                "Select a server. Use arrows to navigate, Enter to select & Esc to quit"
                    .to_string();
            state.selection_index = 0;
        }
        AppAction::TransitionToChat(channel) => {
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
            if let AppState::Editing(_, _, _) = &state.state {
                state.input = state.saved_input.clone().unwrap_or_default();
                state.saved_input = None;
            }

            state.state = AppState::Chatting(channel);
            state.search_input = String::new();
            state.chat_scroll_offset = 0;
            state.cursor_position = 0;
            state.selection_index = 0;
            state.last_typing_sent = None;
            state.status_message =
                "Chatting in channel. Press Enter to send message, Esc to return to channels."
                    .to_string();
        }
        AppAction::TransitionToGuilds => {
            state.input = String::new();
            state.search_input = String::new();
            state.cursor_position = 0;
            state.state = AppState::SelectingGuild;
            state.status_message =
                "Select a server. Use arrows to navigate, Enter to select & Esc to quit"
                    .to_string();
            state.selection_index = 0;
        }
        AppAction::TransitionToDM => {
            state.input = String::new();
            state.search_input = String::new();
            state.cursor_position = 0;
            state.state = AppState::SelectingDM;
            state.status_message =
                "Select a DM. Use arrows to navigate, Enter to select & Esc to quit".to_string();
            state.selection_index = 0;
        }
        AppAction::ApiDeleteMessage(channel_id, message_id) => {
            let api_client_clone = state.api_client.clone();
            let channel_id_clone = channel_id.clone();
            let message_id_clone = message_id.clone();

            tokio::spawn(async move {
                if let Err(e) = api_client_clone
                    .delete_message(&channel_id_clone, &message_id_clone)
                    .await
                {
                    print_log(
                        format!("API Error deleting message: {e}").into(),
                        LogType::Error,
                    )
                    .await
                    .ok();
                }
            });

            // Optimistically remove the message from the local view and track it
            state.deleted_message_ids.insert(message_id.clone());
            state.messages.retain(|m| m.id != message_id);

            // Re-clamp selection index if the list shrank
            if state.selection_index > state.messages.len() {
                state.selection_index = state.messages.len();
            }
        }
        AppAction::ApiEditMessage(channel_id, message_id, content) => {
            let (api_client_clone, channel_id_clone, message_id_clone, content_clone) = (
                state.api_client.clone(),
                channel_id.clone(),
                message_id.clone(),
                content.clone(),
            );

            tokio::spawn(async move {
                if let Err(e) = api_client_clone
                    .edit_message(&channel_id_clone, &message_id_clone, Some(content_clone))
                    .await
                {
                    print_log(
                        format!("API Error editing message: {e}").into(),
                        LogType::Error,
                    )
                    .await
                    .ok();
                }
            });
        }
        AppAction::TransitionToEditing(channel, message, content, c) => {
            let (message_clone, content_clone) = (message.clone(), content.clone());

            state.saved_input = Some(state.input.clone());
            state.input = content.clone();

            if state.vim_mode {
                state.mode = InputMode::Insert;
                if let Some(vim_state) = &mut state.vim_state {
                    vim_state.operator = None;
                    vim_state.pending_keys.clear();
                }
            }

            state.cursor_position = match c {
                'i' | 'I' => 0,
                'a' => content.chars().next().map(|ch| ch.len_utf8()).unwrap_or(0),
                _ => content.len(),
            };

            state.selection_index = 0;
            state.state =
                AppState::Editing(channel.clone(), Box::new(message_clone), content_clone);
            state.status_message =
                "Editing a message in channel. Press Enter to send message. Esc to return to channels"
                    .to_string();
        }
        AppAction::TransitionToHome => {
            state.input = String::new();
            state.search_input = String::new();
            state.cursor_position = 0;
            state.state = AppState::Home;
            state.status_message = "Browse either DMs or Servers. Use arrows to navigate, Enter to select & Esc to quit".to_string();
            state.selection_index = 0;
        }
        AppAction::TransitionToLoading(redirect_state) => {
            state.state = AppState::Loading(redirect_state);
            state.status_message = "Loading...".to_string();
        }
        AppAction::TransitionToLogs => {
            let window = match &state.state {
                AppState::SelectingGuild => Window::Guild,
                AppState::SelectingDM => Window::DM,
                AppState::SelectingChannel(guild) => Window::Channel(guild.clone()),
                AppState::Chatting(ch)
                | AppState::EmojiSelection(ch)
                | AppState::Editing(ch, _, _) => Window::Chat(ch.clone()),
                _ => Window::Home,
            };
            state.state = AppState::Logs(window);
            state.status_message = "Reading Logs".to_string();
        }
        AppAction::EndLoading | AppAction::EndLogs => match &state.state {
            AppState::Loading(redirect) | AppState::Logs(redirect) => match redirect {
                Window::Home => tx_action.send(AppAction::TransitionToHome).await.ok(),
                Window::Guild => tx_action.send(AppAction::TransitionToGuilds).await.ok(),
                Window::DM => tx_action.send(AppAction::TransitionToDM).await.ok(),
                Window::Channel(guild) => tx_action
                    .send(AppAction::TransitionToChannels(guild.clone()))
                    .await
                    .ok(),
                Window::Chat(channel) => tx_action
                    .send(AppAction::TransitionToChat(channel.clone()))
                    .await
                    .ok(),
            },
            _ => None,
        }?,
        AppAction::TransitionToLoadingMessages => {
            state.is_loading = true;
            state.status_message = "Loading Messages...".to_string();
        }
        AppAction::EndLoadingMessages => {
            state.is_loading = false;
            state.status_message =
                "Chatting in channel. Press Enter to send message, Esc to return to channels."
                    .to_string();
        }
        AppAction::DesktopNotification(summary, body, channel_id) => {
            if let Ok(handle) = notify_rust::Notification::new()
                .summary(&summary)
                .body(&body)
                .appname("vimcord")
                .show()
            {
                #[cfg(not(target_os = "windows"))]
                state
                    .active_notifications
                    .entry(channel_id)
                    .or_default()
                    .push(handle);
            }
        }
        AppAction::Tick => {
            state.tick_count = state.tick_count.wrapping_add(1);

            let now = std::time::Instant::now();
            let mut empty_channels = Vec::new();

            for (channel_id, typers) in state.typing_users.iter_mut() {
                typers.retain(|_, timestamp| now.duration_since(*timestamp).as_secs() < 10);
                if typers.is_empty() {
                    empty_channels.push(channel_id.clone());
                }
            }

            for channel_id in empty_channels {
                state.typing_users.remove(&channel_id);
            }

            return Some(KeywordAction::Continue);
        }
        AppAction::NewLogReceived(log) => {
            state.logs.insert(0, log);
            if let AppState::Logs(_) = &state.state
                && state.selection_index > 0
            {
                state.selection_index += 1;
            }
        }
        AppAction::ClearLogs => {
            state.logs.clear();
            if let AppState::Logs(_) = &state.state {
                state.selection_index = 0;
            }
        }
    }

    None
}
