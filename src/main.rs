use std::{env, io, process};

use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, enable_raw_mode},
};
use ratatui::{Terminal, prelude::CrosstermBackend};
use reqwest::{Client, StatusCode};

use crate::{
    api::message::{create_message::create_message, get_channel_messages::get_channel_messages},
    model::{
        channel::{Channel, Message},
        guild::Guild,
    },
};

pub mod api;
pub mod model;

pub type Error = Box<dyn std::error::Error>;

#[derive(Debug)]
enum AppState {
    SelectingGuild,
    SelectingChannel(String),
    Chatting(String),
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
    const ENV_CHANNEL: &str = "DISCORD_CHANNEL";

    let args: Vec<String> = env::args().collect();
    let limit = args.get(1);

    let channel_id = args.get(2).map(|s| s.as_str()).unwrap_or_else(|| {
        env::var(ENV_CHANNEL)
            .unwrap_or_else(|_| {
                eprintln!("Error: DISCORD_CHANNEL variable is missing.");
                process::exit(1);
            })
            .to_string()
            .leak()
    });

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let client = Client::new();

    let token: &str = &env::var(ENV_TOKEN).unwrap_or_else(|_| {
        eprintln!("Error: DISCORD_TOKEN variable is missing.");
        process::exit(1);
    });

    let response = create_message(
        &client,
        channel_id,
        token,
        Some("test rust".to_string()),
        false,
    )
    .await?;

    match response.status() {
        StatusCode::OK | StatusCode::CREATED => {
            println!("[✅ Success] Message sent to {channel_id}!");
            Ok(())
        }
        status => {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "No response body found".to_string());
            Err(format!("Discord API Error: Status {status} - Response Body: {body}").into())
        }
    }
}
