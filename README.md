# kist

A simple terminal torrent client built on [librqbit](https://github.com/ikatson/rqbit) and [ratatui](https://ratatui.rs).

kist keeps things minimal: add a torrent, watch it download, and get out of your way. It runs entirely in the terminal with an adaptive layout that works in small windows too.

## Features

- Add torrents from magnet links, `.torrent` files, or URLs
- Search apibay and download results without leaving the terminal
- DHT support for magnet links
- Session persistence, so your torrent list survives restarts
- Detail pane with overview, files, peers, trackers, and sources tabs
- Attach an arbitrary HTTP source to any torrent as a web seed (BEP 19)
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
| `tab` | Cycle detail tab (overview, files, peers, trackers, sources) |
| `space` | In the files tab, include / exclude the highlighted file |
| `w` | Attach an HTTP web seed to the selected torrent |
| `d` | In the sources tab, detach the highlighted web seed |
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

# Inclusive port range for incoming peer connections. The first free port in
# the range is used; if all of them are taken, the OS picks one instead.
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

# Allow attaching HTTP web seeds to torrents.
enable_web_seeds = true

# Concurrent HTTP requests allowed per web seed.
web_seed_concurrency = 4
```

Session state is stored under the OS data directory (`~/.local/share/kist/session` on Linux), and attached web seeds alongside it in `webseeds.json`.

## Web seeds

Press `w` on a torrent to attach an HTTP or HTTPS URL as a web seed, and the sources tab of the detail pane lists what is attached, how much each has served, and why one failed. This works on torrents you have already added, so you can point a stalled download at a mirror of the same content.

kist follows [BEP 19](https://www.bittorrent.org/beps/bep_0019.html) for the URL layout. A single-file torrent uses the URL as given, or appends the torrent name when the URL ends in `/`. A multi-file torrent fetches each file from `<url>/<name>/<path>`. Data arriving over HTTP is hash-checked exactly like data from the swarm, so a bad mirror costs you a refetch and nothing more.

Because librqbit has no web seed support of its own, kist runs each seed as a loopback BitTorrent peer that answers piece requests with ranged HTTP GETs. So a web seed needs the incoming peer listener to be up, and a seed shows up in the peers tab labelled `web seed`.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
