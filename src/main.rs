use std::{
    collections::{HashMap, HashSet},
    env, io, process,
    sync::Arc,
    time::Duration,
};

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
    api::{
        ApiClient, Channel, Emoji, Guild, Message, PartialMessage, User,
        channel::PermissionContext, dm::DM,
    },
    logs::{LogType, print_log},
    signals::{restore_terminal, setup_ctrlc_handler},
    ui::{draw_ui, handle_input_events, handle_keys_events, vim::VimState},
};

mod api;
mod config;
mod logs;
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
    SelectingChannel(String, String),
    Chatting(String, String),
    EmojiSelection(String, String),
    Editing(String, String, Message, String),
    Loading(Window),
}

#[derive(Debug)]
pub enum AppAction {
    SigInt,
    InputChar(char),
    InputBackspace,
    InputDelete,
    InputEscape,
    InputSubmit,
    SelectNext,
    SelectPrevious,
    SelectLeft,
    SelectRight,
    ApiDeleteMessage(String, String),
    ApiEditMessage(String, String, String),
    ApiUpdateMessages(String, Vec<Message>),
    ApiUpdateChannel(Vec<Channel>),
    ApiUpdateEmojis(Vec<Emoji>),
    ApiUpdateGuilds(Vec<Guild>),
    ApiUpdateDMs(Vec<DM>),
    ApiUpdateContext(Option<PermissionContext>),
    ApiUpdateCurrentUser(User),
    GatewayMessageCreate(Message),
    GatewayMessageUpdate(PartialMessage),
    GatewayMessageDelete(String, String),
    GatewayTypingStart(String, String, Option<String>),
    TransitionToChat(String),
    TransitionToEditing(String, Message, String, char),
    TransitionToChannels(String),
    TransitionToGuilds,
    TransitionToDM,
    TransitionToHome,
    TransitionToLoading(Window),
    TransitionToLoadingMessages,
    EndLoading,
    EndLoadingMessages,
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
    saved_input: Option<String>,
    selection_index: usize,
    status_message: String,
    terminal_height: usize,
    terminal_width: usize,
    emoji_map: Vec<(String, String)>,
    emoji_filter: String,
    emoji_index: usize,
    /// Byte position where the emoji filter started (position of the ':')
    emoji_filter_start: Option<usize>,
    chat_scroll_offset: usize,
    tick_count: usize,
    context: Option<PermissionContext>,
    mode: InputMode,
    cursor_position: usize,
    vim_mode: bool,
    vim_state: Option<VimState>,
    current_user: Option<User>,
    pub last_message_ids: HashMap<String, String>,
    pub discreet_notifs: bool,
    deleted_message_ids: HashSet<String>,
    last_typing_sent: Option<std::time::Instant>,
    typing_users: HashMap<String, HashMap<String, std::time::Instant>>,
    user_names: HashMap<String, String>,
    silent_typing: bool,
    is_loading: bool,
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
        saved_input: None,
        selection_index: 0,
        status_message:
            "Browse either DMs or Servers. Use arrows to navigate, Enter to select & Esc to quit"
                .to_string(),
        terminal_height: 20,
        terminal_width: 80,
        emoji_map: config.emoji_map,
        emoji_filter: String::new(),
        emoji_filter_start: None,
        emoji_index: 0,
        chat_scroll_offset: 0,
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
        deleted_message_ids: HashSet::new(),
        last_typing_sent: None,
        typing_users: HashMap::new(),
        user_names: HashMap::new(),
        silent_typing: config.silent_typing,
        is_loading: false,
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
                    let _ = print_log("Shutdown Program.".into(), LogType::Info);
                    return;
                }
                _ = interval.tick() => {
                    if let Err(e) = tx_ticker.send(AppAction::Tick).await {
                        let _ = print_log(format!("Failed to send tick action: {e}").into(), LogType::Error);
                        return;
                    }
                }
            }
        }
    });

    let input_handle: JoinHandle<Result<(), io::Error>> = tokio::spawn(async move {
        let res = handle_input_events(tx_input, rx_shutdown_input).await;
        if let Err(e) = &res {
            let _ = print_log(format!("Input Error: {e}").into(), LogType::Error);
        }
        res
    });

    let api_state = Arc::clone(&app_state);
    let tx_api = tx_action.clone();
    let mut rx_shutdown_api = tx_shutdown.subscribe();

    let rx_shutdown_gateway = tx_shutdown.subscribe();

    let gateway_token = token.clone();
    let gateway_tx = tx_action.clone();
    let gateway_handle: JoinHandle<()> = tokio::spawn(async move {
        let client = crate::api::GatewayClient::new(gateway_token, gateway_tx);
        if let Err(e) = client.connect(rx_shutdown_gateway).await {
            let _ = print_log(
                format!("Gateway connection failed: {e}").into(),
                LogType::Error,
            );
        }
    });

    let api_handle: JoinHandle<()> = tokio::spawn(async move {
        let api_client_clone;
        {
            let state = api_state.lock().await;
            api_client_clone = state.api_client.clone();
        }

        match api_client_clone.get_current_user().await {
            Ok(user) => {
                if let Err(e) = tx_api.send(AppAction::ApiUpdateCurrentUser(user)).await {
                    let _ = print_log(
                        format!("Failed to send current user update action: {e}").into(),
                        LogType::Error,
                    );
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
                    let _ = print_log(
                        format!("Failed to send guild update action: {e}").into(),
                        LogType::Error,
                    );
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
                    let _ = print_log(
                        format!("Failed to send DM update action: {e}").into(),
                        LogType::Error,
                    );
                }
            }
            Err(e) => {
                let mut state = api_state.lock().await;
                state.status_message = format!("Failed to load DMs. {e}");
            }
        }

        tx_api.send(AppAction::EndLoading).await.ok();

        // Wait for shutdown now since HTTP polling is removed
        let _ = rx_shutdown_api.recv().await;
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

    let _ = tokio::join!(input_handle, api_handle, ticker_handle, gateway_handle);

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenvy::dotenv().ok();
    const ENV_TOKEN: &str = "DISCORD_TOKEN";

    let token: String = env::var(ENV_TOKEN).unwrap_or_else(|_| {
        let msg = "Env Error: DISCORD_TOKEN variable is missing.";
        eprintln!("{msg}");
        let _ = print_log(msg.into(), LogType::Error);
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
