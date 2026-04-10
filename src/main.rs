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
    sync::{Mutex, mpsc},
    task::JoinHandle,
    time,
};

use crate::{
    api::{
        AnyChannel, ApiClient, Channel, Emoji, GatewayClient, Guild, Message, PartialMessage,
        Presence, User,
        channel::PermissionContext,
        dm::DM,
        guild::{GuildMember, PartialGuild},
    },
    logs::{LogReader, LogType, get_log_directory, print_log, watch_logs},
    signals::{restore_terminal, setup_ctrlc_handler},
    ui::{draw_ui, handle_input_events, handle_keys_events, vim::VimState},
};

mod api;
mod config;
mod logs;
mod signals;
mod ui;

const APP_NAME: &str = "vimcord";

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
    Channel(Box<Guild>),
    Chat(Box<AnyChannel>),
}

#[derive(Debug, Clone)]
pub enum AppState {
    Home,
    SelectingGuild,
    SelectingDM,
    SelectingChannel(Box<Guild>),
    Chatting(Box<AnyChannel>),
    EmojiSelection(Box<AnyChannel>),
    Editing(Box<AnyChannel>, Box<Message>, String),
    Loading(Window),
    Logs(Window),
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
    ApiUpdateGuilds(Vec<PartialGuild>),
    ApiUpdateDMs(Vec<DM>),
    ApiUpdateContext(Option<PermissionContext>),
    ApiUpdateCurrentUser(User),
    GatewayMessageCreate(Message),
    GatewayMessageUpdate(PartialMessage),
    GatewayMessageDelete(String, String), // message_id, channel_id
    GatewayTypingStart(String, String, Option<String>), // channel_id, user_id, display_name
    GatewayGuildMembersChunk(String, Vec<GuildMember>, String, String),
    GatewayReadySupplemental(
        std::collections::HashMap<String, String>,
        std::collections::HashMap<String, String>,
    ), // (user_id -> status, user_id -> status_text)
    GatewayPresenceUpdate(Presence), // user_id, status, custom_status_text
    TransitionToChat(Box<AnyChannel>),
    TransitionToEditing(Box<AnyChannel>, Message, String, char),
    TransitionToChannels(Box<Guild>),
    TransitionToGuilds,
    TransitionToDM,
    TransitionToHome,
    TransitionToLoading(Window),
    TransitionToLogs,
    TransitionToLoadingMessages,
    EndLoading,
    EndLogs,
    EndLoadingMessages,
    SelectEmoji,
    Paste(String),
    DesktopNotification(String, String, String),
    Tick,
    NewLogReceived(String),
    ClearLogs,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    Insert,
    Command,
    Search,
}

#[derive(Debug)]
pub struct App {
    api_client: ApiClient,
    gateway_client: GatewayClient,
    state: AppState,
    guilds: Vec<PartialGuild>,
    selected_guild: Option<Guild>,
    guild_members: Vec<GuildMember>,
    channels: Vec<Channel>,
    messages: Vec<Message>,
    custom_emojis: Vec<Emoji>,
    dms: Vec<DM>,
    input: String,
    saved_input: Option<String>,
    search_input: String,
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
    typing_users: HashMap<String, HashMap<String, std::time::Instant>>, // channel_id -> user_id -> timestamp
    user_names: HashMap<String, String>,
    user_statuses: HashMap<String, String>, // user id -> status string (online, offline, etc.)
    user_status_texts: HashMap<String, String>, // user id -> custom status text
    silent_typing: bool,
    is_loading: bool,
    pub active_notifications: HashMap<String, Vec<notify_rust::NotificationHandle>>,
    pub notifs_display_username: bool,
    display_username: bool,
    logs: Vec<String>,
    log_reader: LogReader,
}

pub struct Setup {
    api_client: ApiClient,
    gateway_client: GatewayClient,
    emoji_map: Vec<(String, String)>,
    vim_mode: bool,
    vim_state: Option<VimState>,
    discreet_notifs: bool,
    notifs_display_username: bool,
    silent_typing: bool,
    display_username: bool,
    log_reader: LogReader,
}

impl Default for App {
    fn default() -> Self {
        Self {
            api_client: ApiClient::new(Client::new(), String::new(), DISCORD_BASE_URL.to_string()),
            gateway_client: GatewayClient::default(),
            state: AppState::Loading(Window::Home),
            guilds: Vec::new(),
            selected_guild: None,
            guild_members: Vec::new(),
            channels: Vec::new(),
            messages: Vec::new(),
            custom_emojis: Vec::new(),
            dms: Vec::new(),
            input: String::new(),
            saved_input: None,
            search_input: String::new(),
            selection_index: 0,
            status_message: String::new(),
            terminal_height: 20,
            terminal_width: 80,
            emoji_map: Vec::new(),
            emoji_filter: String::new(),
            emoji_filter_start: None,
            emoji_index: 0,
            chat_scroll_offset: 0,
            tick_count: 0,
            context: None,
            mode: InputMode::Normal,
            cursor_position: 0,
            vim_mode: true,
            vim_state: Some(VimState::default()),
            current_user: None,
            last_message_ids: HashMap::new(),
            discreet_notifs: false,
            deleted_message_ids: HashSet::new(),
            last_typing_sent: None,
            typing_users: HashMap::new(),
            user_names: HashMap::new(),
            user_statuses: HashMap::new(),
            user_status_texts: HashMap::new(),
            silent_typing: false,
            is_loading: false,
            active_notifications: HashMap::new(),
            notifs_display_username: false,
            display_username: false,
            logs: Vec::new(),
            log_reader: LogReader::default(),
        }
    }
}

impl App {
    pub fn setup(values: Setup) -> Self {
        let (
            api_client,
            gateway_client,
            emoji_map,
            vim_mode,
            vim_state,
            discreet_notifs,
            notifs_display_username,
            silent_typing,
            display_username,
            log_reader,
        ) = (
            values.api_client,
            values.gateway_client,
            values.emoji_map,
            values.vim_mode,
            values.vim_state,
            values.discreet_notifs,
            values.notifs_display_username,
            values.silent_typing,
            values.display_username,
            values.log_reader,
        );

        Self {
            api_client,
            gateway_client,
            state: AppState::Loading(Window::Home),
            guilds: Vec::new(),
            selected_guild: None,
            guild_members: Vec::new(),
            channels: Vec::new(),
            messages: Vec::new(),
            custom_emojis: Vec::new(),
            dms: Vec::new(),
            input: String::new(),
            saved_input: None,
            search_input: String::new(),
            selection_index: 0,
            status_message:
                "Browse either DMs or Servers. Use arrows to navigate, Enter to select & Esc to quit"
                    .to_string(),
            terminal_height: 20,
            terminal_width: 80,
            emoji_map,
            emoji_filter: String::new(),
            emoji_filter_start: None,
            emoji_index: 0,
            chat_scroll_offset: 0,
            tick_count: 0,
            context: None,
            mode: InputMode::Normal,
            cursor_position: 0,
            vim_mode,
            vim_state,
            current_user: None,
            last_message_ids: HashMap::new(),
            discreet_notifs,
            deleted_message_ids: HashSet::new(),
            last_typing_sent: None,
            typing_users: HashMap::new(),
            user_names: HashMap::new(),
            user_statuses: HashMap::new(),
            user_status_texts: HashMap::new(),
            silent_typing,
            is_loading: false,
            active_notifications: HashMap::new(),
            notifs_display_username,
            display_username,
            logs: Vec::new(),
            log_reader,
        }
    }
}

async fn run_app(token: String, config: config::Config) -> Result<(), Error> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let vim_mode = config.vim_mode || env::args().any(|arg| arg == "--vim");
    let vim_state = if vim_mode {
        Some(VimState::default())
    } else {
        None
    };

    let (tx_action, mut rx_action) = mpsc::channel::<AppAction>(32);
    let (tx_shutdown, _) = tokio::sync::broadcast::channel::<()>(1);

    let gateway_token = token.clone();
    let gateway_tx = tx_action.clone();
    let gateway_client = GatewayClient::new(gateway_token, gateway_tx);

    let mut path = get_log_directory(APP_NAME).unwrap_or(".".into());
    let _ = std::fs::create_dir_all(&path);
    path.push("logs");

    let log_reader = match LogReader::new(path.clone()) {
        Ok(lg) => lg,
        Err(e) => {
            print_log(
                format!("Failed to create LogReader: {e}").into(),
                LogType::Error,
            )
            .await
            .ok();
            return Err(e);
        }
    };

    let app_state = Arc::new(Mutex::new(App::setup(Setup {
        api_client: ApiClient::new(Client::new(), token.clone(), DISCORD_BASE_URL.to_string()),
        gateway_client,
        emoji_map: config.emoji_map,
        vim_mode,
        vim_state,
        discreet_notifs: config.discreet_notifs,
        notifs_display_username: config.notifs_display_username,
        silent_typing: config.silent_typing,
        display_username: config.display_username,
        log_reader,
    })));

    let tx_ticker = tx_action.clone();
    let mut rx_shutdown_ticker = tx_shutdown.subscribe();

    let ticker_handle: JoinHandle<()> = tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_millis(100));
        loop {
            tokio::select! {
                _ = rx_shutdown_ticker.recv() => {
                    print_log("Shutdown Program.".into(), LogType::Info).await.ok();
                    return;
                }
                _ = interval.tick() => {
                    if let Err(e) = tx_ticker.send(AppAction::Tick).await {
                        print_log(format!("Failed to send tick action: {e}").into(), LogType::Error).await.ok();
                        return;
                    }
                }
            }
        }
    });

    let tx_input = tx_action.clone();
    let rx_shutdown_input = tx_shutdown.subscribe();

    let input_handle: JoinHandle<Result<(), io::Error>> = tokio::spawn(async move {
        let res = handle_input_events(tx_input, rx_shutdown_input).await;
        if let Err(e) = &res {
            print_log(format!("Input Error: {e}").into(), LogType::Error)
                .await
                .ok();
        }
        res
    });

    let tx_logs = tx_action.clone();
    let mut rx_shutdown_logs = tx_shutdown.subscribe();

    let logs_handle: JoinHandle<()> = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = rx_shutdown_logs.recv() => {
                    return;
                }
                Err(e) = watch_logs(path.clone(), tx_logs.clone()) => {
                    print_log(format!("Logs watcher stopped working: {e}").into(), LogType::Error).await.ok();
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    });

    let api_state = Arc::clone(&app_state);
    let tx_api = tx_action.clone();
    let mut rx_shutdown_api = tx_shutdown.subscribe();

    let rx_shutdown_gateway = tx_shutdown.subscribe();

    let client = api_state.lock().await.gateway_client.clone();
    let gateway_handle: JoinHandle<()> = tokio::spawn(async move {
        if let Err(e) = client.connect(rx_shutdown_gateway).await {
            print_log(
                format!("Gateway connection failed: {e}").into(),
                LogType::Error,
            )
            .await
            .ok();
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
                    print_log(
                        format!("Failed to send current user update action: {e}").into(),
                        LogType::Error,
                    )
                    .await
                    .ok();
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
                    print_log(
                        format!("Failed to send guild update action: {e}").into(),
                        LogType::Error,
                    )
                    .await
                    .ok();
                }
            }
            Err(e) => {
                let mut state = api_state.lock().await;
                print_log(
                    format!("Failed to get user guilds action: {e}").into(),
                    LogType::Error,
                )
                .await
                .ok();
                state.status_message = format!("Failed to load servers. {e}");
            }
        }

        match api_client_clone.get_dms().await {
            Ok(dms) => {
                if let Err(e) = tx_api.send(AppAction::ApiUpdateDMs(dms)).await {
                    print_log(
                        format!("Failed to send DM update action: {e}").into(),
                        LogType::Error,
                    )
                    .await
                    .ok();
                }
            }
            Err(e) => {
                let mut state = api_state.lock().await;
                state.status_message = format!("Failed to load DMs. {e}");
            }
        }

        tx_api.send(AppAction::EndLoading).await.ok();

        // Wait for shutdown now since HTTP polling is removed
        rx_shutdown_api.recv().await.ok();
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
                    InputMode::Insert | InputMode::Command | InputMode::Search => {
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

    tx_shutdown.send(()).ok();

    let _ = tokio::join!(
        input_handle,
        api_handle,
        ticker_handle,
        logs_handle,
        gateway_handle
    );

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    print_log("Launching Program...".into(), LogType::Info)
        .await
        .ok();
    dotenvy::dotenv().ok();
    const ENV_TOKEN: &str = "DISCORD_TOKEN";

    let token = match env::var(ENV_TOKEN) {
        Ok(token) => token,
        Err(e) => {
            eprintln!("{e}");
            print_log(e.into(), LogType::Error).await.ok();
            process::exit(1);
        }
    };

    setup_ctrlc_handler();

    let config = config::load_config().await;

    if let Err(e) = run_app(token, config).await {
        restore_terminal();
        return Err(e);
    }

    restore_terminal();

    Ok(())
}
