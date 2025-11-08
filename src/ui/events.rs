use std::io;

use crossterm::event::{self, KeyCode, KeyEventKind};
use tokio::{
    sync::mpsc,
    time::{self, Duration},
};

use crate::AppAction;

pub async fn handle_input_events(
    tx: mpsc::Sender<AppAction>,
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
                                    tx.send(AppAction::InputChar(c)).await.ok();
                                }
                                _ => {}
                            }
                        }
            }
        }
    }
}
