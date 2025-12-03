use std::{env, io, path::PathBuf, process, sync::Arc};

use crossterm::{
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
    time::{self, Duration},
};

use crate::{
    api::{ApiClient, Channel, Emoji, Guild, Message, channel::PermissionContext, dm::DM},
    signals::{restore_terminal, setup_ctrlc_handler},
    ui::{draw_ui, handle_input_events, handle_keys_events},
};

mod api;
mod signals;
mod ui;

const DISCORD_BASE_URL: &str = "https://discord.com/api/v10";
const DEFAULT_EMOJIS_JSON: &str = include_str!("../emojis.json");

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
    TransitionToChat(String),
    TransitionToChannels(String),
    TransitionToGuilds,
    TransitionToDM,
    TransitionToHome,
    TransitionToLoading(Window),
    EndLoading,
    SelectEmoji,
    Tick,
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
    tick_count: usize,
    context: Option<PermissionContext>,
}

impl App {
    fn get_config_path() -> Option<PathBuf> {
        let mut path = dirs::config_dir()?;
        path.push("rivetui");
        path.push("emojis.json");
        Some(path)
    }

    fn load_emoji_map() -> Vec<(String, String)> {
        let config_path = match Self::get_config_path() {
            Some(p) => p,
            None => {
                eprintln!("Error: Could not determine configuration directory.");
                return Vec::new();
            }
        };

        match std::fs::read_to_string(&config_path) {
            Ok(file) => Self::parse_emoji_content(&file),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!(
                    "Configuration file not found, creating default at: {}",
                    config_path.display()
                );

                if let Some(parent) = config_path.parent()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    eprintln!("Error creating configuration directory: {e}");
                    return Self::parse_emoji_content(DEFAULT_EMOJIS_JSON);
                }

                match std::fs::write(&config_path, DEFAULT_EMOJIS_JSON) {
                    Ok(_) => {
                        eprintln!("Default emojis.json created successfully.");
                        Self::parse_emoji_content(DEFAULT_EMOJIS_JSON)
                    }
                    Err(e) => {
                        eprintln!("Error writing default emojis.json: {e}");
                        Self::parse_emoji_content(DEFAULT_EMOJIS_JSON)
                    }
                }
            }
            Err(e) => {
                eprintln!("Error reading configuration file: {e}");
                Self::parse_emoji_content(DEFAULT_EMOJIS_JSON)
            }
        }
    }

    fn parse_emoji_content(content: &str) -> Vec<(String, String)> {
        match serde_json::from_str::<Vec<(String, String)>>(content) {
            Ok(map) => map,
            Err(e) => {
                eprintln!("Error parsing emojis dictionary: {e}");
                Vec::new()
            }
        }
    }
}

async fn run_app(token: String) -> Result<(), Error> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

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
        emoji_map: App::load_emoji_map(),
        emoji_filter: String::new(),
        tick_count: 0,
        context: None,
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

    let _ = tokio::join!(input_handle, api_handle, ticker_handle);

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

    let app_result = run_app(token).await;

    restore_terminal();

    app_result
}
