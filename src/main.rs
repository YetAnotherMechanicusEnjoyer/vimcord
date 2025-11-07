use std::{env, io, process, sync::Arc};

use crossterm::{
    event::{self, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, enable_raw_mode},
};
use ratatui::{
    Terminal,
    crossterm::terminal::disable_raw_mode,
    prelude::CrosstermBackend,
    style::{Style, Stylize},
    widgets::{List, ListItem, ListState},
};
use reqwest::Client;
use tokio::{
    sync::{Mutex, mpsc},
    time::{self, Duration},
};

use crate::{
    api::{
        message::{create_message::create_message, get_channel_messages::get_channel_messages},
        user::get_current_user_guilds::get_current_user_guilds,
    },
    model::{
        channel::{Channel, Message},
        guild::Guild,
    },
};

pub mod api;
pub mod model;

pub type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug)]
enum AppState {
    SelectingGuild,
    SelectingChannel(String),
    Chatting(String),
}

#[derive(Debug)]
enum AppAction {
    Quit,
    InputChar(char),
    InputBackspace,
    InputSubmit,
    SelectNext,
    SelectPrevious,
    ApiUpdateMessages(Vec<Message>),
}

struct App {
    state: AppState,
    guilds: Vec<Guild>,
    channels: Vec<Channel>,
    messages: Vec<Message>,
    input: String,
    selection_index: usize,
    status_message: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenvy::dotenv().ok();
    const ENV_TOKEN: &str = "DISCORD_TOKEN";

    enable_raw_mode().unwrap();
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).unwrap();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    let client = Client::new();

    let token: String = env::var(ENV_TOKEN).unwrap_or_else(|_| {
        eprintln!("Error: DISCORD_TOKEN variable is missing.");
        process::exit(1);
    });

    let app_state = Arc::new(Mutex::new(App {
        state: AppState::SelectingGuild,
        guilds: Vec::new(),
        channels: Vec::new(),
        messages: Vec::new(),
        input: String::new(),
        selection_index: 0,
        status_message: "Loading servers...".to_string(),
    }));

    let (tx_action, mut rx_action) = mpsc::channel::<AppAction>(32);

    let tx_input = tx_action.clone();
    tokio::spawn(async move {
        if let Err(e) = handle_input_events(tx_input).await {
            eprintln!("Input Error: {e}");
        }
    });

    let api_state = Arc::clone(&app_state);
    let api_client = client.clone();
    let api_token = token.clone();

    let mut interval = time::interval(Duration::from_secs(2));

    tokio::spawn(async move {
        match get_current_user_guilds(&api_client, &api_token).await {
            Ok(guilds) => {
                let mut state = api_state.lock().await;
                state.guilds = guilds;
                state.status_message =
                    "Select a server. Use arrows to navigate & Enter to select".to_string();
            }
            Err(e) => {
                api_state.lock().await.status_message = format!("Failed to load servers. {e}");
            }
        }

        loop {
            interval.tick().await;

            let state = api_state.lock().await;
            let current_channel_id = match &state.state {
                AppState::Chatting(id) => Some(id.clone()),
                _ => None,
            };

            if let Some(channel_id) = current_channel_id {
                drop(state);

                match get_channel_messages(
                    &api_client,
                    &channel_id,
                    &api_token,
                    None,
                    None,
                    None,
                    None,
                )
                .await
                {
                    Ok(messages) => {
                        api_state.lock().await.messages = messages;
                    }
                    Err(e) => {
                        api_state.lock().await.status_message = format!("Error loading chat: {e}");
                    }
                }
            }
        }
    });

    fn draw_ui(f: &mut ratatui::Frame, app: &mut App) {
        use ratatui::layout::{Constraint, Direction, Layout};
        use ratatui::widgets::{Block, Borders, Paragraph};

        let area = f.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(90), Constraint::Percentage(10)].as_ref())
            .split(area);

        match &app.state {
            AppState::SelectingGuild => {
                let items: Vec<ListItem> = app
                    .guilds
                    .iter()
                    .map(|g| ListItem::new(g.name.as_str()))
                    .collect();

                let list = List::new(items)
                    .block(
                        Block::default()
                            .title("Servers (Guilds)")
                            .borders(Borders::ALL),
                    )
                    .highlight_style(Style::default().reversed())
                    .highlight_symbol(">> ");

                let mut state = ListState::default().with_selected(Some(app.selection_index));
                f.render_stateful_widget(list, chunks[0], &mut state);
            }
            AppState::SelectingChannel(guild_id) => {
                let title = format!("Channels for Guild: {guild_id}");
                let items: Vec<ListItem> = app
                    .channels
                    .iter()
                    .map(|c| ListItem::new(format!("# {}", c.name)))
                    .collect();

                let list = List::new(items)
                    .block(Block::default().title(title).borders(Borders::ALL))
                    .highlight_style(Style::default().reversed())
                    .highlight_symbol(">> ");

                let mut state = ListState::default().with_selected(Some(app.selection_index));
                f.render_stateful_widget(list, chunks[0], &mut state);
            }
            AppState::Chatting(_) => {
                let main_content = app
                    .messages
                    .iter()
                    .rev()
                    .map(|m| {
                        format!(
                            "[{}] {}: {}",
                            m.timestamp.split('T').nth(1).unwrap_or(""),
                            m.author.username,
                            m.content.as_deref().unwrap_or("(*non-text*)")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                f.render_widget(
                    Paragraph::new(main_content)
                        .block(Block::default().title("Rivet Client").borders(Borders::ALL)),
                    chunks[0],
                );
            }
        };

        f.render_widget(
            Paragraph::new(app.input.as_str()).block(
                Block::default()
                    .title(format!("Input: {}", app.status_message))
                    .borders(Borders::ALL),
            ),
            chunks[1],
        )
    }

    async fn handle_input_events(tx: mpsc::Sender<AppAction>) -> Result<(), io::Error> {
        loop {
            if event::poll(Duration::from_millis(50))? {
                if let event::Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Esc => tx.send(AppAction::Quit).await.unwrap(),
                            KeyCode::Enter => tx.send(AppAction::InputSubmit).await.unwrap(),
                            KeyCode::Backspace => tx.send(AppAction::InputBackspace).await.unwrap(),
                            KeyCode::Up => tx.send(AppAction::SelectPrevious).await.unwrap(),
                            KeyCode::Down => tx.send(AppAction::SelectNext).await.unwrap(),
                            KeyCode::Char(c) => tx.send(AppAction::InputChar(c)).await.unwrap(),
                            _ => {}
                        }
                    }
                }
            }
        }
    }

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
                AppAction::Quit => break,
                AppAction::InputChar(c) => {
                    if let AppState::Chatting(_) = state.state {
                        state.input.push(c);
                    }
                }
                AppAction::InputBackspace => {
                    state.input.pop();
                }
                AppAction::InputSubmit => {
                    let channel_id_clone = if let AppState::Chatting(id) = &state.state {
                        Some(id.clone())
                    } else {
                        None
                    };

                    let content = state.input.drain(..).collect::<String>();

                    let message_data = if content.is_empty() || channel_id_clone.is_none() {
                        None
                    } else {
                        Some((channel_id_clone.unwrap(), content))
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
                AppAction::SelectNext => {
                    match state.state {
                        AppState::SelectingGuild => {
                            if !statte.guild_id..
                        }
                    }
                }
                AppAction::SelectPrevious => {}
                AppAction::ApiUpdateMessages(new_messages) => {
                    state.messages = new_message(s;
                }
            }
        }
    }

    disable_raw_mode().unwrap();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).unwrap();
    terminal.show_cursor().unwrap();
    Ok(())
}
