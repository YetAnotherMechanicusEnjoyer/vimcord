use std::io;

use crossterm::event::{self, KeyCode, KeyEventKind};
use tokio::{
    sync::{MutexGuard, mpsc::Sender},
    time::{self, Duration},
};

use crate::{
    App, AppAction, AppState, KeywordAction,
    api::{Channel, DM, Emoji, Guild},
};

pub async fn handle_input_events(
    tx: Sender<AppAction>,
    mut rx_shutdown: tokio::sync::broadcast::Receiver<()>,
) -> Result<(), io::Error> {
    loop {
        tokio::select! {
            _ = rx_shutdown.recv() => {
                return Ok(());
            }

            _ = time::sleep(Duration::from_millis(50)) => {
                if event::poll(Duration::from_millis(0))?
                    && let event::Event::Key(key) = event::read()? && key.kind == KeyEventKind::Press {
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
                                    match c {
                                        ':' => tx.send(AppAction::SelectEmoji).await.ok(),
                                        c => tx.send(AppAction::InputChar(c)).await.ok(),
                                    };
                                }
                                _ => {}
                            }
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
) -> bool {
    match &state.clone().state {
        AppState::Home => match state.selection_index {
            0 => {
                tx_action.send(AppAction::TransitionToGuilds).await.ok();
            }
            1 => {
                tx_action.send(AppAction::TransitionToDM).await.ok();
            }
            _ => {}
        },
        AppState::SelectingDM => {
            let dms: Vec<&DM> = state
                .dms
                .iter()
                .filter(|d| d.get_name().to_lowercase().contains(&state.input))
                .collect();

            if dms.is_empty() {
                return true;
            }

            let selected_dm = &dms[state.selection_index];
            let dm_id_clone = selected_dm.id.clone();
            let selected_dm_name = selected_dm.recipients[0].username.clone();

            state.input = String::new();
            state.status_message = format!("Loading messages for {selected_dm_name}...");

            tx_action
                .send(AppAction::TransitionToChat(dm_id_clone))
                .await
                .ok();
        }
        AppState::SelectingGuild => {
            let guilds: Vec<&Guild> = state
                .guilds
                .iter()
                .filter(|g| g.name.to_lowercase().contains(&state.input))
                .collect();

            if guilds.is_empty() {
                return true;
            }

            let selected_guild = &guilds[state.selection_index];
            let guild_id_clone = selected_guild.id.clone();
            let selected_guild_name = selected_guild.name.clone();

            let tx_clone = tx_action.clone();

            state.status_message = format!("Loading channels for {selected_guild_name}...");

            let api_client_clone = state.api_client.clone();

            tokio::spawn(async move {
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

                tx_clone
                    .send(AppAction::TransitionToChannels(guild_id_clone))
                    .await
                    .ok();
            });
        }
        AppState::SelectingChannel(_) => {
            let text_channels: Vec<&Channel> = state
                .channels
                .iter()
                .filter(|c| c.channel_type != 4 && c.name.to_lowercase().contains(&state.input))
                .collect();

            if text_channels.is_empty() {
                return true;
            }

            let channel_info = {
                let selected_channel = &text_channels[state.selection_index];
                (selected_channel.id.clone(), selected_channel.name.clone())
            };
            let (channel_id_clone, selected_channel_name) = channel_info;

            state.input = String::new();
            state.status_message = format!("Loading messages for {selected_channel_name}...");

            tx_action
                .send(AppAction::TransitionToChat(channel_id_clone))
                .await
                .ok();
        }
        AppState::EmojiSelection(channel_id) => {
            if state.selection_index < filtered_unicode.len() {
                let (_, char) = filtered_unicode[state.selection_index];

                let emoji_len = state.emoji_filter.len();
                for _ in 0..(emoji_len + 1) {
                    state.input.pop();
                }

                state.input.push_str(char);
                state.input.push(' ');
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

                let emoji_len = state.emoji_filter.len();
                for _ in 0..(emoji_len + 1) {
                    state.input.pop();
                }

                state.input.push_str(&emoji_string);
                state.input.push(' ');
            }

            state.state = AppState::Chatting(channel_id.clone());
            state.emoji_filter.clear();
            state.selection_index = 0;
            state.status_message =
                "Chatting in channel. Press Enter to send message, Esc to retur to channels."
                    .to_string();
        }
        AppState::Chatting(_) => {
            let channel_id_clone = if let AppState::Chatting(id) = &state.state {
                Some(id.clone())
            } else {
                None
            };

            let content = state.input.drain(..).collect::<String>();

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
    false
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
                if n < 0 {
                    state.selection_index = if state.selection_index == 0 {
                        state
                            .channels
                            .iter()
                            .filter(|c| c.channel_type != 4)
                            .count()
                            - n.unsigned_abs() as usize
                    } else {
                        state.selection_index - n.unsigned_abs() as usize
                    };
                } else {
                    state.selection_index = (state.selection_index + n.unsigned_abs() as usize)
                        % state
                            .channels
                            .iter()
                            .filter(|c| c.channel_type != 4)
                            .count();
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
        AppAction::InputEscape => match &state.state {
            AppState::Home => return Some(KeywordAction::Break),
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
        },
        AppAction::InputChar(c) => match &mut state.clone().state {
            AppState::EmojiSelection(channel_id) => {
                state.input.push(c);
                if c == ' ' {
                    state.state = AppState::Chatting(channel_id.clone());
                    state.emoji_filter.clear();
                } else {
                    state.emoji_filter.push(c);
                }
                state.selection_index = 0;
            }
            _ => {
                state.input.push(c);
            }
        },
        AppAction::SelectEmoji => {
            if let AppState::Chatting(channel_id) = &mut state.clone().state {
                let is_start_of_emoji = state.input.ends_with(' ') || state.input.is_empty();

                if is_start_of_emoji {
                    state.input.push(':');
                    let owned_channel_id = channel_id.clone();
                    state.state = AppState::EmojiSelection(owned_channel_id);
                    state.status_message =
                        "Type to filter emoji. Enter to select. Esc to cancel.".to_string();
                    state.emoji_filter.clear();
                    state.selection_index = 0;
                } else {
                    state.input.push(':');
                }
            }
        }
        AppAction::InputBackspace => {
            match &mut state.clone().state {
                AppState::Chatting(_) => {
                    state.input.pop();
                }
                AppState::EmojiSelection(channel_id) => {
                    state.input.pop();
                    state.emoji_filter.pop();
                    if state.emoji_filter.is_empty() {
                        state.state = AppState::Chatting(channel_id.clone());
                        state.status_message = "Chatting in channel. Press Enter to send message. Esc to return channels".to_string();
                    }
                    state.selection_index = 0;
                }
                _ => {
                    state.input.pop();
                }
            }
        }
        AppAction::InputSubmit => {
            if input_submit(
                &mut state,
                &tx_action,
                filtered_unicode,
                filtered_custom,
                total_filtered_emojis,
            )
            .await
            {
                return Some(KeywordAction::Continue);
            }
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
            state.channels = new_channels;
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
        AppAction::TransitionToChannels(guild_id) => {
            state.input = String::new();
            state.state = AppState::SelectingChannel(guild_id);
            state.status_message =
                "Select a server. Use arrows to navigate, Enter to select & Esc to quit"
                    .to_string();
            state.selection_index = 0;
        }
        AppAction::TransitionToChat(channel_id) => {
            state.state = AppState::Chatting(channel_id.clone());
            state.status_message =
                "Chatting in channel. Press Enter to send message, Esc to retur to channels."
                    .to_string();

            if let AppState::EmojiSelection(_) = &state.state {
                if state.input.ends_with(':') {
                    state.input.pop();
                }
                state.emoji_filter.clear();
                state.selection_index = 0;
            }
        }
        AppAction::TransitionToGuilds => {
            state.input = String::new();
            state.state = AppState::SelectingGuild;
            state.status_message =
                "Select a server. Use arrows to navigate, Enter to select & Esc to quit"
                    .to_string();
            state.selection_index = 0;
        }
        AppAction::TransitionToDM => {
            state.input = String::new();
            state.state = AppState::SelectingDM;
            state.status_message =
                "Select a DM. Use arrows to navigate, Enter to select & Esc to quit".to_string();
            state.selection_index = 0;
        }
        AppAction::TransitionToHome => {
            state.input = String::new();
            state.state = AppState::Home;
            state.status_message = "Browse either DMs or Servers. Use arrows to navigate, Enter to select & Esc to quit".to_string();
            state.selection_index = 0;
        }
    }

    None
}
