
## [0.1.0](https://github.com/QaidVoid/kist/compare/v0.0.0...v0.1.0) - 2026-07-15

### ⛰️  Features

- Revamp TUI visuals with adaptive layout and detail tabs - ([374ddb9](https://github.com/QaidVoid/kist/commit/374ddb90cd11c4ffba876e0d56549706b2974177))
- Add scrolling to the torrent detail pane - ([db18102](https://github.com/QaidVoid/kist/commit/db1810296004a2615168e37d7e3a283e9f25c796))
- Update help overlay with all keybindings - ([12427ac](https://github.com/QaidVoid/kist/commit/12427ac9c1e3aa6abee8fb2dac5da7b99ce74b20))
- Add torrent detail pane with files and peers - ([178babe](https://github.com/QaidVoid/kist/commit/178babea86bf8479f62d774c3a757193df82d9b7))
- Sort and filter the torrent list - ([1ffa94d](https://github.com/QaidVoid/kist/commit/1ffa94d503bc07c85f7633606ad79c7992e2f2c1))
- Confirm before removing a torrent - ([46d9fad](https://github.com/QaidVoid/kist/commit/46d9fad0f3f195a58239f48ff483381606477133))
- Render add prompt as a wrapping textbox - ([c09858d](https://github.com/QaidVoid/kist/commit/c09858dd75dbab2b8d7576bfb3278caeacbce540))
- Auto-dismiss status messages after a timeout - ([3f2e105](https://github.com/QaidVoid/kist/commit/3f2e10581f9314a71b86ff7fd9e78b5a88af35e5))
- Cursor editing and horizontal scroll in add bar - ([64d50d1](https://github.com/QaidVoid/kist/commit/64d50d19ca887d165c6582a7abfc95e678b57004))
- Wire runtime, event loop, and terminal setup - ([84aa56d](https://github.com/QaidVoid/kist/commit/84aa56d30a22577d5145e7883bba036e5813b69f))
- Render torrent list, header, overlays - ([dc6dab5](https://github.com/QaidVoid/kist/commit/dc6dab5fb2168d7bf14b3fb0f37e41743e499ea2))
- Add app state machine and key handling - ([4aee136](https://github.com/QaidVoid/kist/commit/4aee1361ac4e2d091e5bba4734269297139412e6))
- Implement librqbit engine wrapper - ([2d3171e](https://github.com/QaidVoid/kist/commit/2d3171e0c02f9c40d35141f6047e2837e2a84123))
- Add config loading, persistence, and CLI - ([94866aa](https://github.com/QaidVoid/kist/commit/94866aa6085e32b76bf19bbe4c79119b5eb2b9fa))
- Add view models and error helpers - ([b708697](https://github.com/QaidVoid/kist/commit/b70869702d04516e6a44d208d135fc29d41a8660))
- Scaffold project deps and module skeleton - ([7ee7df6](https://github.com/QaidVoid/kist/commit/7ee7df66f40c54d91f8e0d948e47ab5dbe257d95))

### 🐛 Bug Fixes

- Persist torrent list across restarts - ([697ea86](https://github.com/QaidVoid/kist/commit/697ea868a77a529f7f3b237f89b2b4e2766f99c8))
- Render remove confirmation as a proper dialog - ([67ef351](https://github.com/QaidVoid/kist/commit/67ef3517cc019dafb203bd26cb6a9e92c7ce2400))
- Preserve shifted chars in add bar input - ([2e855de](https://github.com/QaidVoid/kist/commit/2e855de8ba1e94a1722de499078823d9c57796b1))

### 🚜 Refactor

- Centralize size/speed formatting in format.rs - ([eddd569](https://github.com/QaidVoid/kist/commit/eddd569d7fd398614f33f8f423f56cbbff12e029))

### ⚙️ Miscellaneous Tasks

- Add release automation, crate metadata, and docs - ([62c3c2d](https://github.com/QaidVoid/kist/commit/62c3c2d50b5031343fc8f62de7fface94266af2f))
