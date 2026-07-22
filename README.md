# kist

A simple terminal torrent client built on [librqbit](https://github.com/ikatson/rqbit) and [ratatui](https://ratatui.rs).

kist keeps things minimal: add a torrent, watch it download, and get out of your way. It runs entirely in the terminal with an adaptive layout that works in small windows too.

## Features

- Add torrents from magnet links, `.torrent` files, or URLs
- Search apibay and download results without leaving the terminal
- DHT support for magnet links
- Session persistence, so your torrent list survives restarts
- Detail pane with overview, files, peers, and trackers tabs
- Pause, resume, and remove torrents
- Filter by name and sort by any column
- Adaptive layout that hides low-priority columns in narrow terminals

## Installation

### Prebuilt binaries

Static Linux binaries for x86_64 and aarch64 are available on the [releases page](https://github.com/QaidVoid/kist/releases), along with BLAKE3 checksums and build provenance attestations.

### From crates.io

```sh
cargo install kist
```

### From source

```sh
git clone https://github.com/QaidVoid/kist
cd kist
cargo install --path .
```

## Usage

```sh
kist                          # start the UI
kist <magnet|file|url>        # add a torrent on startup
kist --download-dir <DIR>     # override the download directory for this run
kist --config <PATH>          # use an alternate config file
```

## Keybindings

Press `?` inside kist to see this list at any time.

| Key | Action |
| --- | --- |
| `a` | Add a torrent |
| `A` | Add with options (start paused, output folder, pick files) |
| `f` | Search indexers (`enter` downloads the selected result) |
| `j` / `k` | Move down / up |
| `i` | Open / close torrent details |
| `tab` | Cycle detail tab (overview, files, peers, trackers) |
| `space` | In the files tab, include / exclude the highlighted file |
| `ctrl+d` / `ctrl+u` | Scroll detail content (also `pgdn` / `pgup`) |
| `g` / `G` | Detail top / bottom (also `home` / `end`) |
| `p` / `space` | Pause selected |
| `r` | Resume selected |
| `enter` | Toggle pause / resume |
| `d` | Remove (asks to confirm) |
| `f` / `D` | In the confirm dialog: forget (keep files) / delete with files |
| `/` | Filter by name (blank clears) |
| `L` | Set global rate limits (`down up`, e.g. `2M 512K`; `-` clears) |
| `s` | Cycle sort column |
| `S` | Reverse sort direction |
| `?` | Toggle help |
| `q`, `ctrl+c` | Quit |

`esc` cancels prompts and closes the detail pane.

## Configuration

kist reads a TOML config file from the OS config directory (`~/.config/kist/config.toml` on Linux). A default file is written on first run. All fields are optional; missing fields fall back to their defaults.

```toml
# Where downloaded torrents are written (defaults to the OS download folder).
download_directory = "/home/you/Downloads"

# Inclusive port range for incoming peer connections.
listen_ports = [6881, 6889]

# Global speed caps as human sizes (e.g. "2M", "512K"); omit for unlimited.
# These can also be changed live with the `L` key.
download_limit = "2M"
upload_limit = "512K"

# Enable the DHT (needed for magnet links).
enable_dht = true

# Persist the torrent list across restarts.
enable_session_persistence = true

# UI refresh interval in milliseconds.
refresh_interval_ms = 250
```

Session state is stored under the OS data directory (`~/.local/share/kist/session` on Linux).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
