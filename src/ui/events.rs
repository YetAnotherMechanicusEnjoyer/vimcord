use std::io;

use crossterm::event::{self, KeyCode, KeyEventKind};
use reqwest::Client;
use tokio::{
    sync::{MutexGuard, mpsc::Sender},
    time::{self, Duration},
};

use crate::{
    App, AppAction, AppState, KeywordAction, UNICODE_EMOJI_DICTIONARY,
    api::{create_message, get_channel, get_guild_channels, get_guild_emojis},
    model::{Channel, Emoji},
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
                    && let event::Event::Key(key) = event::read()?
                        && key.kind == KeyEventKind::Press {
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
                                    if c == ':' {
                                        tx.send(AppAction::SelectEmoji).await.ok();
                                    } else {
                                        tx.send(AppAction::InputChar(c)).await.ok();
                                    }
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
    client: &Client,
    token: String,
    tx_action: &Sender<AppAction>,
    filtered_unicode: Vec<(&str, &str)>,
    filtered_custom: Vec<&Emoji>,
    total_filtered_emojis: usize,
) -> bool {
    match &state.clone().state {
        AppState::SelectingGuild => {
            if state.guilds.is_empty() {
                return true;
            }

            let selected_guild = &state.guilds[state.selection_index];
            let guild_id_clone = selected_guild.id.clone();
            let selected_guild_name = selected_guild.name.clone();

            let client_clone = client.clone();
            let token_clone = token.clone();
            let tx_clone = tx_action.clone();

            state.status_message = format!("Loading channels for {selected_guild_name}...");

            tokio::spawn(async move {
                match get_guild_channels(&client_clone, &token_clone, &guild_id_clone).await {
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
                match get_guild_emojis(&client_clone, &guild_id_clone, &token_clone).await {
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
                .filter(|c| c.channel_type != 4)
                .collect();

            if text_channels.is_empty() {
                return true;
            }

            let channel_info = {
                let selected_channel = &text_channels[state.selection_index];
                (selected_channel.id.clone(), selected_channel.name.clone())
            };
            let (channel_id_clone, selected_channel_name) = channel_info;

            state.state = AppState::Chatting(channel_id_clone.clone());
            state.status_message = format!("Chatting in channel #{selected_channel_name}");
            state.selection_index = 0;
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
                let client_clone = client.clone();
                let token_clone = token.clone();

                tokio::spawn(async move {
                    match create_message(
                        &client_clone,
                        &channel_id_clone,
                        &token_clone,
                        Some(content),
                        false,
                    )
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
    client: &Client,
    token: String,
    tx_action: Sender<AppAction>,
) -> Option<KeywordAction> {
    let filtered_unicode: Vec<(&str, &str)> = UNICODE_EMOJI_DICTIONARY
        .iter()
        .filter(|(name, _)| name.starts_with(&state.emoji_filter))
        .copied()
        .collect();

    let state_clone = state.clone();
    let filtered_custom: Vec<&Emoji> = state_clone
        .custom_emojis
        .iter()
        .filter(|e| e.name.starts_with(&state.emoji_filter))
        .collect();

    let total_filtered_emojis = filtered_unicode.len() + filtered_custom.len();

    match action {
        AppAction::InputEscape => match &state.state {
            AppState::SelectingGuild => {
                return Some(KeywordAction::Break);
            }
            AppState::SelectingChannel(_) => {
                state.state = AppState::SelectingGuild;
                state.status_message =
                    "Select a server. Use arrows to navigate, Enter to select & Esc to quit"
                        .to_string();
                state.selection_index = 0;
            }
            AppState::Chatting(channel_id) => {
                let channel = get_channel(client, &token, &channel_id.clone())
                    .await
                    .unwrap();
                match channel.guild_id {
                    Some(guild_id) => {
                        state.state = AppState::SelectingChannel(guild_id);
                        state.status_message =
                            "Select a server. Use arrows to navigate, Enter to select & Esc to quit"
                                .to_string();
                        state.selection_index = 0;
                    }
                    None => {
                        state.state = AppState::SelectingGuild;
                        state.status_message =
                            "Select a server. Use arrows to navigate, Enter to select & Esc to quit"
                                .to_string();
                        state.selection_index = 0;
                    }
                }
            }
            AppState::EmojiSelection(channel_id) => {
                state.state = AppState::Chatting(channel_id.clone());
                if state.input.ends_with(':') {
                    state.input.pop();
                }
                state.emoji_filter.clear();
                state.selection_index = 0;
            }
        },
        AppAction::InputChar(c) => match &mut state.state {
            AppState::Chatting(_) | AppState::SelectingGuild => {
                state.input.push(c);
            }
            AppState::EmojiSelection(_) => {
                state.emoji_filter.push(c);
                state.selection_index = 0;
            }
            _ => {}
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
                    state.emoji_filter.pop();
                    if state.emoji_filter.is_empty() {
                        state.state = AppState::Chatting(channel_id.clone());
                        if state.input.ends_with(':') {
                            state.input.pop();
                        }
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
                client,
                token.clone(),
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
        AppAction::TransitionToChannels(guild_id) => {
            state.state = AppState::SelectingChannel(guild_id);
            state.status_message =
                "Select a channel. Use arrows to navigate, Enter to select & Esc to quit"
                    .to_string();
            state.selection_index = 0;
        }
        AppAction::TransitionToChat(channel_id) => {
            state.state = AppState::Chatting(channel_id);
            state.status_message = "Chatting...".to_string();
        }
        AppAction::TransitionToGuilds => {
            state.state = AppState::SelectingGuild;
            state.status_message =
                "Select a server. Use arrows to navigate, Enter to select & Esc to quit"
                    .to_string();
            state.selection_index = 0;
        }
    }

    None
}
