# Zenless Torrent

A calm, fast BitTorrent client for Windows, part of the **Zenless** suite
(separate from Zenless Download Manager). Built in Rust with
[eframe/egui](https://github.com/emilk/egui) and the
[librqbit](https://github.com/ikatson/rqbit) BitTorrent engine, without any C code.

Website: <https://zenless-suite.vercel.app> · Downloads: <https://zenless-suite.vercel.app/download>

## Features

- **Adding torrents:** magnet links (dialog, or <kbd>Ctrl</kbd>+<kbd>V</kbd> anywhere in the window),
  `.torrent` files (file picker, drag and drop, command line) and the Zenless browser extensions
  through the local API.
- **Add torrent dialog:** a spinner while magnet metadata comes in from peers, then name, size, piece
  info, a file tree with tri-state checkboxes and All/None buttons, the save folder and "Start immediately".
- **Main table:** Name, Size, Progress (bar + %), Status (Downloading, Seeding, Paused, Checking,
  Metadata, Error, Completed), Down, Up, Peers, ETA, Ratio, Added. Columns sort and resize. Rows support
  multi-select (<kbd>Ctrl</kbd>/<kbd>Shift</kbd>+click, <kbd>Ctrl</kbd>+<kbd>A</kbd>), and there's a
  right-click menu. The empty state shows hints.
- **Actions:** pause and resume (per torrent or all), remove, remove and delete the files (after a
  confirmation), open the folder (a double-click does this too), copy the magnet link or info hash,
  force a recheck.
- **Sidebar filters with counts:** All, Downloading, Seeding, Completed, Paused, Error. The live global
  speed and a sparkline sit at the bottom.
- **Details panel:** General (info hash, save path, pieces, sizes, speeds, ETA, errors), Files (per-file
  progress; changing the checkboxes updates the selection live), Peers (address, client, TCP/uTP, per-peer
  speeds and totals), Trackers (plus DHT and LSD state), Speed (download and upload graph).
- **Session persistence** in `%APPDATA%\Zenless\Torrent\session`: torrents come back after a restart,
  with fast resume. Also DHT, a configurable listen port (a busy port falls back to a random one), UPnP
  port forwarding, optional µTP, and download/upload speed limits applied live.
- **Settings:** default folder, start with Windows, start minimized, completion notifications, and a
  "Make Zenless Torrent the default for magnet links and .torrent files" button, which writes the HKCU
  keys and has a shortcut to Windows' Default apps. The Network and Browser integration pages cover the
  API status, the last extension contact and install steps. Appearance comes from the shared Zenless
  theme gallery (13 themes plus custom themes). There's also an About page.
- **Notifications:** a toast appears when a download completes, and the taskbar flashes when the window
  isn't focused.
- **Single instance:** a second launch forwards its magnet or `.torrent` arguments to the running app and
  exits.

## Local API (browser integration)

Zenless Torrent listens on `http://127.0.0.1:6813` (loopback only):

| Request | Result |
|---|---|
| `GET /ping` | `{"ok":true,"app":"zenless-torrent","name":"Zenless Torrent","version":"0.1.0"}` |
| `GET /status` | counts (`downloading`, `seeding`, `paused`, `total`), `download_speed`, `upload_speed`, `items` |
| `POST /add` | exactly one of `magnet`, `torrent_url` (+ `cookies`, `referrer`, `user_agent`), `torrent_base64` (+ `name`), `path`; optional `source`, `mode` = `ask` (default: shows the Add dialog and brings the window forward) or `start` |
| `POST /focus` | restore and focus the window |
| `POST /quit` | save and exit (only when there's no `Origin` header) |

Security rules:

- An `Origin` header, if present, must be a browser-extension origin; any other origin gets `403`.
- Every `POST` needs an `X-Zenless-Client` header; without it the request gets `400`.
- `OPTIONS` preflights get the CORS headers.
- `path` is accepted only from local programs.
- Bodies are limited to 8 MiB.

```bash
curl -s http://127.0.0.1:6813/ping
curl -s -X POST http://127.0.0.1:6813/add -H "X-Zenless-Client: curl/1.0" \
     -H "Content-Type: application/json" \
     -d '{"magnet":"magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Big+Buck+Bunny","mode":"start"}'
```

## Command line

```
zenless-torrent.exe [--minimized] [magnet:?... | path\to\file.torrent ...]
```

## Building

You need a stable Rust toolchain (edition 2024). Standard builds use MSVC:

```bash
cargo build --release          # target/release/zenless-torrent.exe
cargo test
```

The dependency tree is pure Rust: TLS goes through SChannel via `native-tls`, and SHA-1 uses the Windows
crypto API via `crypto-hash`, so `ring`, `aws-lc` and `openssl` aren't needed. `build.rs` embeds the icon,
the version info and the manifest. It uses `zig rc` when zig is on `PATH`, and `embed-resource` otherwise.

### Demo mode (screenshots)

`ZENLESS_DEMO=1` fills the window with sample torrents. It doesn't start a session, doesn't use the
network and doesn't write anything to disk. `ZENLESS_DEMO_VIEW` opens an extra state; the values are
`add`, `adding`, `magnet`, `settings`, `settings-network`, `settings-browser`, `settings-appearance`,
`about`, `files`, `peers`, `trackers`, `speed`, `remove`, `empty` and `toast`. `ZENLESS_DEMO_THEME=Paper`
(or any other built-in theme name) applies a theme in memory only, without writing the shared
`appearance.json`. Combine these with `ZENLESS_SCREENSHOT=out.png` to save a PNG of the window and exit:

```bash
ZENLESS_DEMO=1 ZENLESS_DEMO_VIEW=add ZENLESS_SCREENSHOT=add.png target/release/zenless-torrent.exe
```

## Files

| Path | Contents |
|---|---|
| `%APPDATA%\Zenless\Torrent\settings.json` | settings |
| `%APPDATA%\Zenless\Torrent\torrents.json` | added/completed times, lifetime upload, magnets still resolving |
| `%APPDATA%\Zenless\Torrent\session\` | librqbit session (torrent files, fast-resume bitfields, DHT state) |
| `%APPDATA%\Zenless\Torrent\zenless-torrent.log` | log (`ZENLESS_LOG=debug` for more detail) |
| `%APPDATA%\Zenless\appearance.json`, `themes\` | shared Zenless theme settings |

## License

MIT, Copyright (c) 2026 Zenless. librqbit is licensed under Apache-2.0.
