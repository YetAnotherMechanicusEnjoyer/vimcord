# vimcord Roadmap & Feature Backlog

This document outlines the missing features required to make vimcord a feature-complete Discord client. It serves as a directive for the project's development over the coming years.

## Current Project Capabilities
*Based on the current state of the codebase:*
- **Basic Navigation**: Navigating between Guilds, Channels, and Direct Messages.
- **Message Viewing**: Fetching and displaying text messages in channels and DMs.
- **Message Sending**: Sending basic text messages to channels.
- **Permissions**: Basic permission calculation for viewing channels.
- **UI**: A TUI with Vim-like keybindings, input handling, and basic rendering.

---

## Missing Discord Features

The following features are grouped by **Importance** (Foundation, Critical, High, Medium, Low). Within each group, they are sorted by **Expected Implementation Difficulty** (from Easiest to Hardest), considering the constraints of a Rust TUI app.

### 0. Foundation (Blockers)
*These are infrastructural tasks that must be completed before other features can be fully realized.*
1. **WebSocket / Gateway Migration** *(Difficulty: Hard)*
   - **Description**: Refactor the application network layer away from relying solely on HTTP requests. Move to persistent WebSocket connections. **This is a foundational prerequisite.** Many other features (such as Proper Push Notifications, Typing Indicators, Presence Updates, and Slash Commands) are currently blocked because they require a WebSocket connection to be implemented correctly.
   - **Implementation**: Connect to the Discord Gateway via WebSockets to receive real-time push events (e.g., new messages, status updates, typing indicators) rather than making HTTP API calls or polling.

### 1. Critical Importance (Core Usability)
*These are essential features that users expect from any functioning chat client. Without these, the client feels incomplete.*
1. <small>~~**Message Deletion** *(Difficulty: Easy)*~~</small>
   - <small>~~**Description**: Ability for the user to delete their own messages.~~</small>
   - <small>~~**Implementation**: Adding a keybinding in the UI to call `DELETE /channels/{channel.id}/messages/{message.id}`.~~</small>
2. **Mark as Read / Read Receipts** *(Difficulty: Easy)*
   - **Description**: Updating the local and remote "last read" state to clear unread notification badges.
   - **Implementation**: Hitting the `/ack` endpoint for channels when viewed.
3. **Proper Push Notifications** *(Difficulty: Medium) (🔒 Blocked by WebSockets)*
   - **Description**: Replace the current hacky workaround for notifications with reliable, instant desktop push notifications for new messages.
   - **Implementation**: Listen for `MESSAGE_CREATE` events in real-time over the WebSocket Gateway to trigger native notifications correctly without missing any or double-notifying.
4. **Message Editing** *(Difficulty: Medium)*
   - **Description**: Ability to edit existing sent messages.
   - **Implementation**: Needs a UI mode to load old text into the input buffer and a `PATCH /channels/{channel.id}/messages/{message.id}` request.
5. **Message Replies** *(Difficulty: Medium)*
   - **Description**: In-line replying to specific messages.
   - **Implementation**: UI interaction to select a message, and adding `message_reference` to the message creation API payload.
6. **Mentions & Pings** *(Difficulty: Medium)*
   - **Description**: Highlighting mentions natively (`@username`), alerting the user, and providing autocomplete for users in the input bar.
   - **Implementation**: Parsing `<@id>` tags in text rendering and querying guild members for autocomplete.

### 2. High Importance (Standard Discord Experience)
*Features that make Discord unique and are heavily used in daily communication.*

1. **User Status/Presence Update** *(Difficulty: Easy) (🔒 Blocked by WebSockets)*
   - **Description**: Setting custom status text or changing presence (Online, Idle, DND, Invisible).
   - **Implementation**: Sending Gateway presence update payloads directly or via a simple UI modal.
2. **Pinned Messages** *(Difficulty: Easy)*
   - **Description**: A dedicated UI panel to view and jump to messages pinned in a channel.
   - **Implementation**: Fetching from the pins endpoint and rendering a static list.
3. **Typing Indicators** *(Difficulty: Easy) (🔒 Blocked by WebSockets)*
   - **Description**: Showing "User is typing..." when someone is active in the current channel.
   - **Implementation**: Listening to Gateway events (`TYPING_START`) and displaying a transient UI element.
4. **Reactions** *(Difficulty: Medium)*
   - **Description**: Viewing, adding, and removing Unicode and custom emoji reactions on messages.
   - **Implementation**: UI for selecting emojis, rendering reaction counts under messages, and hitting the reactions endpoints.
5. **Emoji Handling Refactor** *(Difficulty: Medium)*
   - **Description**: Refactor away from using a static `emojis.json` file for emoji definition/lookups.
   - **Implementation**: Migrate to a proper Rust crate for handling emojis (such as `emojis` or `unicode-emoji`). This ensures up-to-date and robust parsing, categorization, and handling of emoji shortcodes without maintaining a localized JSON dump.
6. **Threads** *(Difficulty: Medium)*
   - **Description**: Viewing thread lists, joining threads, and sending messages within threads.
   - **Implementation**: Threads are conceptually similar to channels but require specific API handling and a nested or distinct UI view mode.
7. **Vim Command Mode & Quitting** *(Difficulty: Hard)*
   - **Description**: Stop `Esc` from immediately quitting the entire application. Instead, introduce a true Command Mode where users must type `:q` (or `:quit`) to exit, more closely mirroring actual Vim behavior.
   - **Implementation**: Intercept `Esc` to ensure it only drops to Normal Mode (or clears input). Add an input buffer for handling `:` commands and parse them accordingly. **Note: This will require a new UI overlay/input mode and will likely significantly overlap and conflict with emoji (`:emoji:`) shortcode handling.**
7. **Embeds & Attachments Viewing** *(Difficulty: Impractical)*
   - **Description**: Displaying rich embeds, links, and text summaries of images/files in the TUI.
   - **Implementation**: Parsing complex embed payloads and formatting them nicely in the terminal. (Full image rendering requires terminal graphics protocols like Sixel).

### 3. Medium Importance (Organization & Power User Tools)
*Features geared towards server management, moderation, and advanced usage.*

1. **Moderation Actions** *(Difficulty: Medium)*
   - **Description**: Tools to kick, ban, or timeout users from the TUI.
   - **Implementation**: Simple API calls but requires context menu UI when selecting a user and entering reasons.
2. **Channel Management** *(Difficulty: Medium)*
   - **Description**: Creating, editing, and deleting text/voice channels.
   - **Implementation**: Forms/modals in the UI to submit channel metadata.
3. **Guild Roles & Settings** *(Difficulty: Medium)*
   - **Description**: Viewing, creating, and assigning roles, changing nicknames.
   - **Implementation**: UI forms and interaction with the guild member/role endpoints.
4. **File Uploads** *(Difficulty: Hard)*
   - **Description**: Sending attachments alongside messages.
   - **Implementation**: Requires `multipart/form-data` requests and a file picker interface in the terminal.
5. **Slash Commands / Interactions** *(Difficulty: Hard) (🔒 Blocked by WebSockets)*
   - **Description**: Support for executing application commands (`/commands`).
   - **Implementation**: Highly complex. Requires fetching command lists, parsing command options dynamically in the UI, and handling interaction payloads over the Gateway.
6. **Message Search** *(Difficulty: Hard)*
   - **Description**: Searching messages within a guild or channel.
   - **Implementation**: Needs a dedicated search input UI, handling pagination, and a results view panel.

### 4. Low Importance (Niche or TUI Limitations)
*Features that are either rarely used by the core demographic or are fundamentally difficult/impractical for a lightweight Terminal UI.*

1. **Soundboard** *(Difficulty: Medium)*
   - **Description**: Playing server soundboard sounds.
   - **Implementation**: Requires integrating an external audio playback library.
2. **Voice Channels** *(Difficulty: Very Hard)*
   - **Description**: Connecting to voice channels to send/receive audio.
   - **Implementation**: Requires complex WebRTC, UDP implementations, and audio device management. Pushes the boundaries of a simple TUI client.
3. **Stickers** *(Difficulty: Impractical)*
   - **Description**: Rendering Discord stickers.
   - **Implementation**: Very difficult to visually represent in a standard text terminal without fallback to mere URLs or text descriptions.
4. **Video/Screen Sharing** *(Difficulty: Impractical) (🔒 Blocked by WebSockets)*
   - **Description**: Viewing or broadcasting video streams.
   - **Implementation**: Essentially impossible to cleanly achieve in a standard terminal environment without delegating to a GUI tool.
