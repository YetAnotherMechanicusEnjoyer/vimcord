use std::{collections::HashMap, env, io, process, sync::Arc, time::Duration};

use crossterm::{
    cursor::SetCursorStyle,
    event::EnableBracketedPaste,
    execute,
    terminal::{EnterAlternateScreen, enable_raw_mode},
};
use ratatui::{Terminal, prelude::CrosstermBackend};
use reqwest::Client;
use tokio::{
    sync::{
        Mutex,
        mpsc::{self},
    },
    task::JoinHandle,
    time::{self},
};

use crate::{
    api::{ApiClient, Channel, Emoji, Guild, Message, User, channel::PermissionContext, dm::DM},
    signals::{restore_terminal, setup_ctrlc_handler},
    ui::{draw_ui, handle_input_events, handle_keys_events, vim::VimState},
};

mod api;
mod config;
mod signals;
mod ui;

const DISCORD_BASE_URL: &str = "https://discord.com/api/v10";

pub type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug)]
pub enum KeywordAction {
    Continue,
    Break,
}

#[derive(Debug, Clone)]
pub enum Window {
    Home,
    Guild,
    DM,
    Channel(String),
    Chat(String),
}

#[derive(Debug, Clone)]
pub enum AppState {
    Home,
    SelectingGuild,
    SelectingDM,
    SelectingChannel(String),
    Chatting(String),
    EmojiSelection(String),
    Loading(Window),
}

#[derive(Debug)]
pub enum AppAction {
    SigInt,
    InputChar(char),
    InputBackspace,
    InputEscape,
    InputSubmit,
    SelectNext,
    SelectPrevious,
    ApiUpdateMessages(Vec<Message>),
    ApiUpdateChannel(Vec<Channel>),
    ApiUpdateEmojis(Vec<Emoji>),
    ApiUpdateGuilds(Vec<Guild>),
    ApiUpdateDMs(Vec<DM>),
    ApiUpdateContext(Option<PermissionContext>),
    ApiUpdateCurrentUser(User),
    ApiUpdateUnreadMessages(String, Vec<Message>),
    TransitionToChat(String),
    TransitionToChannels(String),
    TransitionToGuilds,
    TransitionToDM,
    TransitionToHome,
    TransitionToLoading(Window),
    EndLoading,
    SelectEmoji,
    Paste(String),
    Tick,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    Insert,
}

#[derive(Debug, Clone)]
pub struct App {
    api_client: ApiClient,
    state: AppState,
    guilds: Vec<Guild>,
    channels: Vec<Channel>,
    messages: Vec<Message>,
    custom_emojis: Vec<Emoji>,
    dms: Vec<DM>,
    input: String,
    selection_index: usize,
    status_message: String,
    terminal_height: usize,
    terminal_width: usize,
    emoji_map: Vec<(String, String)>,
    emoji_filter: String,
    /// Byte position where the emoji filter started (position of the ':')
    emoji_filter_start: Option<usize>,
    tick_count: usize,
    context: Option<PermissionContext>,
    mode: InputMode,
    cursor_position: usize,
    vim_mode: bool,
    vim_state: Option<VimState>,
    current_user: Option<User>,
    last_message_ids: HashMap<String, String>,
    discreet_notifs: bool,
}

async fn run_app(token: String, config: config::Config) -> Result<(), Error> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let vim_mode = config.vim_mode || env::args().any(|arg| arg == "--vim");

    let app_state = Arc::new(Mutex::new(App {
        api_client: ApiClient::new(Client::new(), token.clone(), DISCORD_BASE_URL.to_string()),
        state: AppState::Loading(Window::Home),
        guilds: Vec::new(),
        channels: Vec::new(),
        messages: Vec::new(),
        custom_emojis: Vec::new(),
        dms: Vec::new(),
        input: String::new(),
        selection_index: 0,
        status_message:
            "Browse either DMs or Servers. Use arrows to navigate, Enter to select & Esc to quit"
                .to_string(),
        terminal_height: 20,
        terminal_width: 80,
        emoji_map: config.emoji_map,
        emoji_filter: String::new(),
        emoji_filter_start: None,
        tick_count: 0,
        context: None,
        mode: InputMode::Normal,
        cursor_position: 0,
        vim_mode,
        vim_state: if vim_mode {
            Some(VimState::default())
        } else {
            None
        },
        current_user: None,
        last_message_ids: HashMap::new(),
        discreet_notifs: config.discreet_notifs,
    }));

    let (tx_action, mut rx_action) = mpsc::channel::<AppAction>(32);
    let (tx_shutdown, _) = tokio::sync::broadcast::channel::<()>(1);

    let tx_input = tx_action.clone();
    let rx_shutdown_input = tx_shutdown.subscribe();

    let mut rx_shutdown_ticker = tx_shutdown.subscribe();
    let tx_ticker = tx_action.clone();

    let ticker_handle: JoinHandle<()> = tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_millis(100));
        loop {
            tokio::select! {
                _ = rx_shutdown_ticker.recv() => {
                    return;
                }
                _ = interval.tick() => {
                    if let Err(e) = tx_ticker.send(AppAction::Tick).await {
                        eprintln!("Failed to send tick action: {e}");
                        return;
                    }
                }
            }
        }
    });

    let input_handle: JoinHandle<Result<(), io::Error>> = tokio::spawn(async move {
        let res = handle_input_events(tx_input, rx_shutdown_input).await;
        if let Err(e) = &res {
            eprintln!("Input Error: {e}");
        }
        res
    });

    let api_state = Arc::clone(&app_state);
    let tx_api = tx_action.clone();
    let mut rx_shutdown_api = tx_shutdown.subscribe();

    let mut interval = time::interval(Duration::from_secs(2));

    let api_handle: JoinHandle<()> = tokio::spawn(async move {
        let api_client_clone;
        {
            let state = api_state.lock().await;
            api_client_clone = state.api_client.clone();
        }

        match api_client_clone.get_current_user().await {
            Ok(user) => {
                if let Err(e) = tx_api.send(AppAction::ApiUpdateCurrentUser(user)).await {
                    eprintln!("Failed to send current user update action: {e}");
                }
            }
            Err(e) => {
                let mut state = api_state.lock().await;
                state.status_message = format!("Failed to load current user. {e}");
            }
        }

        match api_client_clone.get_current_user_guilds().await {
            Ok(guilds) => {
                if let Err(e) = tx_api.send(AppAction::ApiUpdateGuilds(guilds)).await {
                    eprintln!("Failed to send guild update action: {e}");
                }
            }
            Err(e) => {
                let mut state = api_state.lock().await;
                state.status_message = format!("Failed to load servers. {e}");
            }
        }

        match api_client_clone.get_dms().await {
            Ok(dms) => {
                if let Err(e) = tx_api.send(AppAction::ApiUpdateDMs(dms)).await {
                    eprintln!("Failed to send DM update action: {e}");
                }
            }
            Err(e) => {
                let mut state = api_state.lock().await;
                state.status_message = format!("Failed to load DMs. {e}");
            }
        }

        tx_api.send(AppAction::EndLoading).await.ok();

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

                        match api_client_clone.get_channel_messages(
                            &channel_id,
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

    let background_state = Arc::clone(&app_state);
    let tx_background = tx_action.clone();
    let mut rx_shutdown_background = tx_shutdown.subscribe();
    let mut background_interval = time::interval(Duration::from_secs(3));

    let background_handle: JoinHandle<()> = tokio::spawn(async move {
        let api_client_clone;
        {
            let state = background_state.lock().await;
            api_client_clone = state.api_client.clone();
        }

        // Wait a bit before starting background loops so we don't clobber initial requests
        tokio::time::sleep(Duration::from_secs(5)).await;

        loop {
            tokio::select! {
                _ = rx_shutdown_background.recv() => {
                    return;
                }

                _ = background_interval.tick() => {
                    // Fetch DMs to get the latest last_message_id for each
                    if let Ok(dms) = api_client_clone.get_dms().await {
                        let mut dms_to_fetch = Vec::new();

                        {
                            let state = background_state.lock().await;

                            for dm in dms {
                                if let Some(new_last_id) = &dm.last_message_id {
                                    // Only fetch messages if we know we have a new message
                                    let should_fetch = match state.last_message_ids.get(&dm.id) {
                                        Some(tracked_id) => new_last_id.parse::<u64>().unwrap_or_default() > tracked_id.parse::<u64>().unwrap_or_default(),
                                        None => true,
                                    };

                                    if should_fetch {
                                        dms_to_fetch.push(dm.id.clone());
                                    }
                                }
                            }
                        }

                        for channel_id in dms_to_fetch {
                            if let Ok(messages) = api_client_clone.get_channel_messages(
                                &channel_id,
                                None,
                                None,
                                None,
                                Some(5),
                            ).await
                                && let Err(e) = tx_background.send(AppAction::ApiUpdateUnreadMessages(channel_id, messages)).await
                            {
                                eprintln!("Failed to send unread message update action: {e}");
                                return;
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

            if !state_guard.vim_mode {
                execute!(io::stdout(), SetCursorStyle::BlinkingBar).ok();
            } else {
                match state_guard.mode {
                    InputMode::Normal => {
                        execute!(io::stdout(), SetCursorStyle::BlinkingBlock).ok();
                    }
                    InputMode::Insert => {
                        execute!(io::stdout(), SetCursorStyle::BlinkingBar).ok();
                    }
                }
            }
        }
        if let Some(action) = rx_action.recv().await {
            let state = app_state.lock().await;

            match handle_keys_events(state, action, tx_action.clone()).await {
                Some(KeywordAction::Continue) => continue,
                Some(KeywordAction::Break) => break,
                None => {}
            }
        }
    }

    drop(rx_action);

    let _ = tx_shutdown.send(());

    let _ = tokio::join!(input_handle, api_handle, ticker_handle, background_handle);

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenvy::dotenv().ok();
    const ENV_TOKEN: &str = "DISCORD_TOKEN";

    let token: String = env::var(ENV_TOKEN).unwrap_or_else(|_| {
        eprintln!("Env Error: DISCORD_TOKEN variable is missing.");
        process::exit(1);
    });

    setup_ctrlc_handler();

    let config = config::load_config();

    if let Err(e) = run_app(token, config).await {
        restore_terminal();
        return Err(e);
    }

    restore_terminal();

    Ok(())
}
