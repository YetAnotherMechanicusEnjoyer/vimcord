# Rivet

![Chat Example](./readme/Rivet.png)

<p align="center">
  <a href="https://aur.archlinux.org/packages/rivetui">
    <img src="https://img.shields.io/aur/version/rivetui?style=for-the-badge&logo=Arch-Linux&logoColor=white&color=1793D1"></a>
  <a href="https://crates.io/crates/rivetui">
    <img src="https://img.shields.io/crates/v/rivetui?style=for-the-badge&logo=rust&logoColor=white&color=FF4E00"></a>
  <a href="https://crates.io/crates/rivetui">
    <img src="https://img.shields.io/crates/d/rivetui?style=for-the-badge&logo=crates-io&logoColor=white&color=D07300"></a>
  <a href="https://github.com/YetAnotherMechanicusEnjoyer/Rivet/blob/5392a5b9f8982187b02d11ccd94dcd952fee36b6/LICENSE">
    <img src="https://img.shields.io/github/license/YetAnotherMechanicusEnjoyer/Rivet?style=for-the-badge&logo=github&color=2EA44F"></a>
</p>
<p align="center">
  <a href="https://github.com/YetAnotherMechanicusEnjoyer/Rivet/actions/workflows/aur.yml">
    <img src="https://img.shields.io/github/actions/workflow/status/YetAnotherMechanicusEnjoyer/Rivet/aur.yml?style=for-the-badge&logo=github-actions&logoColor=white&color=1793D1&label=AUR"></a>
  <a href="https://github.com/YetAnotherMechanicusEnjoyer/Rivet/actions/workflows/rust.yml">
    <img src="https://img.shields.io/github/actions/workflow/status/YetAnotherMechanicusEnjoyer/Rivet/rust.yml?style=for-the-badge&logo=github-actions&logoColor=white&color=FF4E00&label=Rust"></a>
  <a href="https://github.com/YetAnotherMechanicusEnjoyer/Rivet/actions/workflows/bin.yml">
    <img src="https://img.shields.io/github/actions/workflow/status/YetAnotherMechanicusEnjoyer/Rivet/bin.yml?style=for-the-badge&logo=github-actions&logoColor=white&color=2088FF&label=Binaries"></a>
</p>

## About

Rivet is a terminal UI Discord client written in Rust.

For best visuals, install  [NerdFonts](https://www.nerdfonts.com/)

### Terms of Service Notice

> [!WARNING]
> Under no circumstances should you use a Discord User Token (also known as a self-bot token) with this software or any associated tools.

Discord's [Terms of Service (ToS)](https://discord.com/terms) explicitly and strictly prohibit the use of User Tokens for programmatic access or self-botting. Violation of these terms can lead to permanent termination of your Discord account.

> **Do not use self-bots or user-bots.** Each account must be associated with a human, not a bot. Self-bots put strain on Discord’s infrastructure and our ability to run our services. For more information, you can read our Developer Policies [here](https://discord.com/developers/docs/policy).

The developers, contributors, and maintainers of this repository disclaim all liability and responsibility for any and all consequences that may arise from a user's decision to violate Discord's Terms of Service.

> [!CAUTION]
> We do not encourage, endorse, or support the use of User Tokens.
> The user assumes all risk for any account actions, bans, or penalties issued by Discord due to improper use of this software.

## Installation ([AUR](https://aur.archlinux.org/packages/rivetui))

> [!IMPORTANT]
> Make sure to have [YaY](https://github.com/Jguer/yay) installed.

```bash
yay -S rivetui
# or
yay -S rivetui-git
```

## Installation (Binaries)

> [!NOTE]
> Download the binaries from the [releases](https://github.com/YetAnotherMechanicusEnjoyer/Rivet/releases/)

## Installation ([Cargo](https://doc.rust-lang.org/cargo/))

### Dependencies

> [!IMPORTANT]
> Make sure to have [Rust](https://www.rust-lang.org/tools/install) installed.

### Compilation from [crates.io](https://crates.io/crates/rivetui)

> [!IMPORTANT]
> Make sure that `~/.cargo/bin` is in your PATH env variable.

```bash
cargo install rivetui
```

### Compilation from source

> [!NOTE]
> Clone the repo somewhere and compile the program.

```bash
git clone https://github.com/YetAnotherMechanicusEnjoyer/Rivet
cd Rivet/
cargo build --release
```

> [!TIP]
> Either execute the binary `./target/release/rivetui` or put it inside a directory included in your PATH env variable.

## Initialization

> [!NOTE]
> Either make a `.env` file at the root of the repository that contains the `DISCORD_TOKEN` variable, save it in your shell env or write it with the command.

> [!TIP]
> Exemple of a `.env` file :

```env
DISCORD_TOKEN="your-token-here"
```

> [!TIP]
> Exemple of a shell env variable :

```env
export DISCORD_TOKEN="your-token-here"
```

> [!TIP]
> Exemple of a command-line env variable :

```env
DISCORD_TOKEN="your-token-here" rivetui
```

## Usage

```bash
rivetui
```

or

```env
DISCORD_TOKEN="your-token-here" rivetui
```

## Licence

[![MIT](https://img.shields.io/github/license/YetAnotherMechanicusEnjoyer/Rivet?style=for-the-badge&logo=github&color=2EA44F)](https://github.com/YetAnotherMechanicusEnjoyer/Rivet/blob/5392a5b9f8982187b02d11ccd94dcd952fee36b6/LICENSE)
