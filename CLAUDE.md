# jf400

Jellyfin terminal client in Rust (ratatui) with an AS/400 5250 look. Plays through an external `mpv` process.

## Commands

- Build: `cargo build`
- Run: `cargo run --release` (needs a real TTY, `mpv`, and a Jellyfin server)
- Render check: `cargo test screens -- --nocapture` draws sign-on, menu, and queue screens into a test buffer and prints them. Use it to check layout without a server.

## Layout

- `src/main.rs`: terminal setup and the event loop (`drain` → `tick` → `draw` → key poll, 200 ms).
- `src/app.rs`: all state and input handling. Screens, list stack, queue, commands, function keys, progress reporting, media key handling.
- `src/ui.rs`: drawing only. Writes straight into the ratatui buffer with `put()`. Colors are consts at the top.
- `src/api.rs`: blocking Jellyfin client (`reqwest::blocking`), `Item` model, `Query` enum, paging, playback reports.
- `src/player.rs`: spawns mpv with `--input-ipc-server` and talks JSON over the Unix socket. A reader thread keeps `PlayerState` up to date through `observe_property`.
- `src/media.rs`: MPRIS through `souvlaki` (D-Bus).
- `src/config.rs`: `~/.config/jf400/config.json` (server, user, token, device id, insecure flag).

## How things fit

- **Threads, not async.** Network calls run in short-lived threads and report back over an `mpsc` channel (`Msg`). `App::drain` applies them on the UI thread. Never block the UI thread on the network.
- **Progress reports** go through one long-lived reporter thread so start/progress/stop stay in order. Each report carries a `Client` clone, so it always has the current token. Call `App::shutdown` before exit so the final stop is sent.
- **The queue lives in two places.** `App::queue` holds the `Item`s and mpv holds the playlist. They are matched by index (`playlist-pos`). Any change to one must change the other.
- **Paging.** List queries fetch `PAGE` (200) items. `App::load_more` appends the next page when the cursor is within 50 rows of the end. The renderer copies out only the visible rows each frame.
- **Synthetic rows.** Items with `kind` starting `X:` are menu rows built by the app (Artists / Albums / Songs), not server objects.
- **Options in lists.** Digits fill the `Opt` column only while the command line is empty. Once it holds text, everything goes to the command line.

## Conventions

- UI text is uppercase for messages (AS/400 style), for example `NO RECORDS FOUND`.
- Keep the 5250 look: green (`GREEN`), white for titles, turquoise for hints, blue for the F-key bar, red for errors. Do not add rounded borders or emoji.
- New Jellyfin calls go in `api.rs`. Return `Result<_, String>` with a short uppercase message for user-facing errors.
- The menu is `MENU` in `app.rs`; menu numbers are matched in `run_command`. Keep the two in sync.

## Not done yet

Shuffle/repeat, favorites and mark-as-played, transcoding, album art, mpv crash detection, wide-character column alignment, automated tests beyond the render check.
