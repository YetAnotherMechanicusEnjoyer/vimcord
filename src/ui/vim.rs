use std::time::Instant;
use tokio::sync::{MutexGuard, mpsc::Sender};

use crate::{App, AppAction, InputMode};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VimOperator {
    Delete,
    Change,
    Yank,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VimMotion {
    WordForward,
    WordBackward,
    Line,
    CharRight,
    CharLeft,
    StartOfLine,
    EndOfLine,
}

#[derive(Debug, Clone)]
pub struct VimState {
    pub operator: Option<VimOperator>,
    pub pending_keys: String,
    pub last_action_time: Instant,
}

impl Default for VimState {
    fn default() -> Self {
        Self {
            operator: None,
            pending_keys: String::new(),
            last_action_time: Instant::now(),
        }
    }
}

pub fn clamp_cursor(state: &mut MutexGuard<'_, App>) {
    let len = state.input.len();
    if len == 0 {
        state.cursor_position = 0;
    } else if state.cursor_position >= len {
        let last_char_len = state
            .input
            .chars()
            .last()
            .map(|c| c.len_utf8())
            .unwrap_or(0);
        state.cursor_position = len.saturating_sub(last_char_len);
    }
}

fn get_motion_range(state: &MutexGuard<'_, App>, motion: VimMotion) -> (usize, usize) {
    let start = state.cursor_position;
    let len = state.input.len();
    let input = &state.input;

    let end = match motion {
        VimMotion::WordForward => {
            let mut pos = start;
            if let Some(c) = input[pos..].chars().next() {
                if c.is_whitespace() {
                    // Cursor is on whitespace: skip whitespace only, stop at start of next word
                    while pos < len {
                        if let Some(c) = input[pos..].chars().next() {
                            if c.is_whitespace() {
                                pos += c.len_utf8();
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                } else {
                    // Cursor is on a word/non-whitespace: skip rest of this word...
                    while pos < len {
                        if let Some(c) = input[pos..].chars().next() {
                            if !c.is_whitespace() {
                                pos += c.len_utf8();
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    // ...then skip following whitespace to land at start of next word
                    while pos < len {
                        if let Some(c) = input[pos..].chars().next() {
                            if c.is_whitespace() {
                                pos += c.len_utf8();
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
            }
            pos.min(len)
        }
        VimMotion::WordBackward => {
            let mut pos = start;
            if pos == 0 {
                return (start, 0);
            }

            // First, check what character is immediately before the cursor
            let prev_char = input[..pos].chars().next_back();

            if let Some(c) = prev_char {
                if c.is_whitespace() {
                    // We're after whitespace - skip all whitespace backwards
                    while pos > 0 {
                        if let Some(c) = input[..pos].chars().next_back() {
                            if c.is_whitespace() {
                                pos -= c.len_utf8();
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    // Now skip the word backwards to find its beginning
                    while pos > 0 {
                        if let Some(c) = input[..pos].chars().next_back() {
                            if !c.is_whitespace() {
                                pos -= c.len_utf8();
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                } else {
                    // We're after a word character - check if we're at the start of a word
                    // by looking at the character before that
                    let two_back = if pos >= c.len_utf8() {
                        input[..pos - c.len_utf8()].chars().next_back()
                    } else {
                        None
                    };

                    if two_back.is_none() || two_back.is_some_and(|c2| c2.is_whitespace()) {
                        // At start of word - move to previous word
                        pos -= c.len_utf8(); // Move past the first char of current word
                        // Skip whitespace backwards
                        while pos > 0 {
                            if let Some(c) = input[..pos].chars().next_back() {
                                if c.is_whitespace() {
                                    pos -= c.len_utf8();
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                        // Skip the previous word backwards
                        while pos > 0 {
                            if let Some(c) = input[..pos].chars().next_back() {
                                if !c.is_whitespace() {
                                    pos -= c.len_utf8();
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    } else {
                        // In middle of word - go to start of current word
                        while pos > 0 {
                            if let Some(c) = input[..pos].chars().next_back() {
                                if !c.is_whitespace() {
                                    pos -= c.len_utf8();
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
            pos
        }
        VimMotion::Line => len, // Special case, usually handled by operator logic
        VimMotion::CharRight => start + input[start..].chars().next().map_or(0, |c| c.len_utf8()),
        VimMotion::CharLeft => {
            start
                - input[..start]
                    .chars()
                    .next_back()
                    .map_or(0, |c| c.len_utf8())
        }
        VimMotion::StartOfLine => 0,
        VimMotion::EndOfLine => len,
    };

    (start, end)
}

fn execute_operator(state: &mut MutexGuard<'_, App>, operator: VimOperator, range: (usize, usize)) {
    let (start, end) = range;
    let (low, high) = if start < end {
        (start, end)
    } else {
        (end, start)
    };

    match operator {
        VimOperator::Delete => {
            if high > low && state.input.is_char_boundary(low) && state.input.is_char_boundary(high)
            {
                state.input.drain(low..high);
                state.cursor_position = low;
            }
        }
        VimOperator::Change => {
            // Not implemented yet
        }
        VimOperator::Yank => {
            // Not implemented yet
        }
    }
}

pub async fn handle_vim_keys(
    mut state: MutexGuard<'_, App>,
    c: char,
    tx_action: Sender<AppAction>,
) {
    // Check for timeout
    if let Some(vim_state) = &mut state.vim_state {
        if vim_state.operator.is_some() {
            if Instant::now()
                .duration_since(vim_state.last_action_time)
                .as_secs()
                >= 1
            {
                vim_state.operator = None;
                vim_state.pending_keys.clear();
            }
        }
    }

    // Ensure vim_state exists (it should, but for safety)
    if state.vim_state.is_none() {
        state.vim_state = Some(VimState::default());
    }

    // We need to clone some state to avoid borrow checker issues when calling async functions
    // or when mutating state later.
    let current_operator = state.vim_state.as_ref().unwrap().operator;

    match c {
        'i' => {
            state.mode = InputMode::Insert;
        }
        'I' => {
            state.cursor_position = 0;
            state.mode = InputMode::Insert;
        }
        'a' => {
            if let Some(c) = state.input[state.cursor_position..].chars().next() {
                state.cursor_position += c.len_utf8();
            }
            state.mode = InputMode::Insert;
        }
        'A' => {
            state.cursor_position = state.input.len();
            state.mode = InputMode::Insert;
        }
        'j' => {
            tx_action.send(AppAction::SelectNext).await.ok();
        }
        'k' => {
            tx_action.send(AppAction::SelectPrevious).await.ok();
        }
        'h' => {
            if let Some(c) = state.input[..state.cursor_position].chars().next_back() {
                state.cursor_position -= c.len_utf8();
            }
        }
        'l' => {
            if let Some(c) = state.input[state.cursor_position..].chars().next() {
                let next_pos = state.cursor_position + c.len_utf8();
                if next_pos < state.input.len() {
                    state.cursor_position = next_pos;
                }
            }
        }
        'w' => {
            if let Some(op) = current_operator {
                let range = get_motion_range(&state, VimMotion::WordForward);
                execute_operator(&mut state, op, range);
                if let Some(vim_state) = &mut state.vim_state {
                    vim_state.operator = None;
                }
            } else {
                let (_, end) = get_motion_range(&state, VimMotion::WordForward);
                state.cursor_position = end;
                clamp_cursor(&mut state);
            }
        }
        'b' => {
            if let Some(op) = current_operator {
                let range = get_motion_range(&state, VimMotion::WordBackward);
                execute_operator(&mut state, op, range);
                if let Some(vim_state) = &mut state.vim_state {
                    vim_state.operator = None;
                }
            } else {
                let (_, end) = get_motion_range(&state, VimMotion::WordBackward);
                state.cursor_position = end;
            }
        }
        'd' => {
            if let Some(VimOperator::Delete) = current_operator {
                // dd case
                state.input.clear();
                state.cursor_position = 0;
                if let Some(vim_state) = &mut state.vim_state {
                    vim_state.operator = None;
                }
            } else {
                if let Some(vim_state) = &mut state.vim_state {
                    vim_state.operator = Some(VimOperator::Delete);
                    vim_state.last_action_time = Instant::now();
                }
            }
        }
        'x' => {
            let pos = state.cursor_position;
            if pos < state.input.len() && state.input.is_char_boundary(pos) {
                if let Some(ch) = state.input[pos..].chars().next() {
                    let char_end = pos + ch.len_utf8();
                    state.input.drain(pos..char_end);
                    clamp_cursor(&mut state);
                }
            }
        }
        ':' => {
            // In the future, this could enter command mode.
            // For now, we do nothing to avoid conflict with standard Vim behavior.
        }
        _ => {
            if let Some(vim_state) = &mut state.vim_state {
                vim_state.operator = None;
                vim_state.pending_keys.clear();
            }
        }
    }
}
