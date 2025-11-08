use std::{env, io, process, sync::Arc};

use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, enable_raw_mode},
};
use ratatui::{Terminal, prelude::CrosstermBackend};
use reqwest::Client;
use tokio::{
    sync::{
        Mutex, MutexGuard,
        mpsc::{self, Sender},
    },
    task::JoinHandle,
    time::{self, Duration},
};

use crate::{
    api::{
        guild::get_guild_channels::get_guild_channels,
        message::{create_message::create_message, get_channel_messages::get_channel_messages},
        user::get_current_user_guilds::get_current_user_guilds,
    },
    model::{
        channel::{Channel, Message},
        guild::Guild,
    },
    signals::{restore_terminal, setup_ctrlc_handler},
    ui::{draw::draw_ui, events::handle_input_events},
};

pub mod api;
pub mod model;
mod signals;
pub mod ui;

pub type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug)]
enum AppState {
    SelectingGuild,
    SelectingChannel(String),
    Chatting(String),
}

#[derive(Debug)]
pub enum AppAction {
    InputChar(char),
    InputBackspace,
    InputEscape,
    InputSubmit,
    SelectNext,
    SelectPrevious,
    ApiUpdateMessages(Vec<Message>),
    ApiUpdateChannel(Vec<Channel>),
    #[allow(dead_code)]
    TransitionToChat(String),
    TransitionToChannels(String),
    #[allow(dead_code)]
    TransitionToGuilds,
}

pub struct App {
    state: AppState,
    guilds: Vec<Guild>,
    channels: Vec<Channel>,
    messages: Vec<Message>,
    input: String,
    selection_index: usize,
    status_message: String,
    terminal_height: usize,
    terminal_width: usize,
}

async fn input_submit(
    state: &mut MutexGuard<'_, App>,
    client: &Client,
    token: String,
    tx_action: &Sender<AppAction>,
) -> bool {
    match &state.state {
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
                        tx_clone
                            .send(AppAction::TransitionToChannels(guild_id_clone))
                            .await
                            .ok();
                    }
                    Err(e) => {
                        eprintln!("Failed to load channels: {e}");
                    }
                }
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

async fn move_selection(state: &mut MutexGuard<'_, App>, n: i32) {
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
                    state.selection_index += n.unsigned_abs() as usize % state.guilds.len();
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
        _ => {}
    }
}

async fn run_app(token: String) -> Result<(), Error> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let client = Client::new();

    let app_state = Arc::new(Mutex::new(App {
        state: AppState::SelectingGuild,
        guilds: Vec::new(),
        channels: Vec::new(),
        messages: Vec::new(),
        input: String::new(),
        selection_index: 0,
        status_message: "Loading servers...".to_string(),
        terminal_height: 20,
        terminal_width: 80,
    }));

    let (tx_action, mut rx_action) = mpsc::channel::<AppAction>(32);
    let (tx_shutdown, _) = tokio::sync::broadcast::channel::<()>(1);

    let tx_input = tx_action.clone();
    let rx_shutdown_input = tx_shutdown.subscribe();

    let input_handle: JoinHandle<Result<(), io::Error>> = tokio::spawn(async move {
        let res = handle_input_events(tx_input, rx_shutdown_input).await;
        if let Err(e) = &res {
            eprintln!("Input Error: {e}");
        }
        res
    });

    let api_state = Arc::clone(&app_state);
    let api_client = client.clone();
    let api_token = token.clone();
    let tx_api = tx_action.clone();
    let mut rx_shutdown_api = tx_shutdown.subscribe();

    let mut interval = time::interval(Duration::from_secs(1));

    let api_handle: JoinHandle<()> = tokio::spawn(async move {
        match get_current_user_guilds(&api_client, &api_token).await {
            Ok(guilds) => {
                let mut state = api_state.lock().await;
                state.guilds = guilds;
                state.status_message =
                    "Select a server. Use arrows to navigate, Enter to select & Esc to quit."
                        .to_string();
            }
            Err(e) => {
                api_state.lock().await.status_message = format!("Failed to load servers. {e}");
            }
        }

        loop {
            tokio::select! {
                _ = rx_shutdown_api.recv() => {
                    return;
                }

                _ = interval.tick() => {
                    let current_channel_id = {
                        let state = api_state.lock().await;
                        match &state.state {
                            AppState::Chatting(id) => Some(id.clone()),
                            _ => None,
                        }
                    };

                    if let Some(channel_id) = current_channel_id {
                        const MESSAGE_LIMIT: usize = 100;

                        match get_channel_messages(
                            &api_client,
                            &channel_id,
                            &api_token,
                            None,
                            None,
                            None,
                            Some(MESSAGE_LIMIT),
                        )
                        .await
                        {
                            Ok(messages) => {
                                if let Err(e) = tx_api.send(AppAction::ApiUpdateMessages(messages)).await {
                                    eprintln!("Failed to send message update action: {e}");
                                    return;
                                }
                            }
                            Err(e) => {
                                api_state.lock().await.status_message = format!("Error loading chat: {e}");
                            }
                        }
                    }
                }
            }
        }
    });

    loop {
        {
            let mut state_guard = app_state.lock().await;
            terminal
                .draw(|f| {
                    draw_ui(f, &mut state_guard);
                })
                .unwrap();
        }
        if let Some(action) = rx_action.recv().await {
            let mut state = app_state.lock().await;

            match action {
                AppAction::InputEscape => match state.state {
                    AppState::SelectingGuild => {
                        break;
                    }
                    AppState::SelectingChannel(_) => {
                        state.state = AppState::SelectingGuild;
                        state.status_message = "Select a server. Use arrows to navigate, Enter to select & Esc to quit".to_string();
                        state.selection_index = 0;
                    }
                    AppState::Chatting(_) => {
                        state.state = AppState::SelectingGuild;
                        state.status_message = "Select a server. Use arrows to navigate, Enter to select & Esc to quit".to_string();
                        state.selection_index = 0;
                    }
                },
                AppAction::InputChar(c) => {
                    if let AppState::Chatting(_) = state.state {
                        state.input.push(c);
                    }
                }
                AppAction::InputBackspace => {
                    state.input.pop();
                }
                AppAction::InputSubmit => {
                    if input_submit(&mut state, &client, token.clone(), &tx_action).await {
                        continue;
                    }
                }
                AppAction::SelectNext => move_selection(&mut state, 1).await,
                AppAction::SelectPrevious => move_selection(&mut state, -1).await,
                AppAction::ApiUpdateMessages(new_messages) => {
                    state.messages = new_messages;
                }
                AppAction::ApiUpdateChannel(new_channels) => {
                    state.channels = new_channels;
                    let text_channels_count = state.channels.len();
                    if text_channels_count > 0 {
                        state.status_message =
                            "Channels loaded. Select one to chat. (Esc to return to Servers)"
                                .to_string();
                    } else {
                        state.status_message =
                            "No text channels found. (Esc to return to Servers)".to_string();
                    }
                    state.selection_index = 0;
                }
                AppAction::TransitionToChannels(guild_id) => {
                    state.state = AppState::SelectingChannel(guild_id);
                    state.selection_index = 0;
                }
                AppAction::TransitionToChat(channel_id) => {
                    state.state = AppState::Chatting(channel_id);
                    state.status_message = "Chatting...".to_string();
                }
                AppAction::TransitionToGuilds => {
                    state.state = AppState::SelectingGuild;
                    state.selection_index = 0;
                }
            }
        }
    }

    drop(rx_action);

    let _ = tx_shutdown.send(());

    let _ = tokio::join!(input_handle, api_handle);

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenvy::dotenv().ok();
    const ENV_TOKEN: &str = "DISCORD_TOKEN";

    let token: String = env::var(ENV_TOKEN).unwrap_or_else(|_| {
        eprintln!("Error: DISCORD_TOKEN variable is missing.");
        process::exit(1);
    });

    setup_ctrlc_handler();

    let app_result = run_app(token).await;

    restore_terminal();

    app_result
}
