# k9mux

[![ci](https://github.com/nimishgj/k9mux/actions/workflows/ci.yml/badge.svg)](https://github.com/nimishgj/k9mux/actions/workflows/ci.yml)

Run k9s against several clusters at once.

k9mux doesn't reimplement k9s. Every pane is a real `k9s` process on its own pty;
k9mux arranges them into tmux-style windows and panes, points each one at a
different cluster, and stays out of the way. What you see inside a pane is k9s —
same keys, same views, same version you already have installed.

```
k9mux                                         # $KUBECONFIG, or ~/.kube/config
k9mux --kubeconfig ~/.kube/config-prod        # a specific file
KUBECONFIG=~/.kube/a:~/.kube/b k9mux          # several, merged
```

Requires `k9s` on your PATH.

## Install

Every [release](https://github.com/nimishgj/k9mux/releases/latest) carries
prebuilt binaries for macOS and Linux, on both Apple silicon / arm64 and x86.
The Linux builds are static, so they run on any distribution.

```sh
V=0.1.0
case "$(uname -sm)" in
  "Darwin arm64")  T=aarch64-apple-darwin ;;
  "Darwin x86_64") T=x86_64-apple-darwin ;;
  "Linux aarch64") T=aarch64-unknown-linux-musl ;;
  "Linux x86_64")  T=x86_64-unknown-linux-musl ;;
esac
curl -sSfL "https://github.com/nimishgj/k9mux/releases/download/v$V/k9mux-$V-$T.tar.gz" | tar xz
sudo install -m755 "k9mux-$V-$T/k9mux" /usr/local/bin/k9mux
```

Each tarball has a `.sha256` next to it. On macOS, a tarball downloaded by a
browser is quarantined: `xattr -dr com.apple.quarantine k9mux` before running.

From source, with a Rust toolchain:

```sh
cargo install --git https://github.com/nimishgj/k9mux    # or: make install
```

Building and releasing it yourself is in
[docs/releasing.md](docs/releasing.md).

It opens one window called `k9mux` on your first cluster. `⌥space %` splits it
so a second cluster sits alongside, `⌥space c` opens another window, `⌥space ,`
names it after whatever you are actually doing.

📖 **[Usage](docs/usage.md)** · **[Configuration](docs/configuration.md)**

## Keys

Every keystroke goes to the k9s in the focused pane, so nothing in k9s is
shadowed — not `:`, not `%`, not `ctrl-a`. k9mux' own commands sit behind a
prefix, `option-space` by default. Moving between panes needs no prefix.

| key | |
|---|---|
| `⌥h` `⌥j` `⌥k` `⌥l` | move focus left / down / up / right |
| `⌥space` `c` | new window |
| `⌥space` `,` | rename the window |
| `⌥space` `n` `p` | next / previous window |
| `⌥space` `%` `"` | split the focused pane, side-by-side / stacked |
| `⌥space` `x` | close the pane |
| `⌥space` `<` `>` | previous / next cluster in the focused pane |
| `⌥space` `H` `J` `K` `L` | resize the pane (or drag its border with the mouse) |
| `⌥space` `z` | zoom the focused pane |
| `⌥space` `w` | pick a cluster by name |
| `⌥space` `g` | a window per cluster group |
| `⌥space` `e` | collapse the sidebar |
| `⌥space` `r` | restart k9s in the pane |
| `⌥space` `?` | help |
| `⌥space` `q` | quit k9mux |

All of it is rebindable — `k9mux --keys` prints the defaults in config syntax.
The full list is in [docs/usage.md](docs/usage.md).

Quitting k9s in a pane with `:q` closes that pane and the others expand to fill
the space. Closing a window's last pane closes the window; closing the last
window quits k9mux. A k9s that *crashes* keeps its pane so the error stays
readable, and `⌥space r` restarts it.

## Config

`~/.config/k9mux/config`, in the style of `tmux.conf`:

```sh
set prefix C-b

bind '|' split-vertical
bind -n C-Left focus-left
unbind e

kubeconfig ~/.kube/config-prod
cluster prod-us use1-a use1-b
```

Without any `cluster` lines, each kubeconfig file becomes one cluster named
after the file. Full reference in
[docs/configuration.md](docs/configuration.md), and `config.example` is a
copyable starting point.

## Layout

| | |
|---|---|
| `src/session.rs` | a child process on a pty, its screen parsed by `vt100` |
| `src/app.rs` | windows, panes, focus, which cluster each pane runs |
| `src/keymap.rs` | key specs, bindings, prefix resolution |
| `src/keys.rs` | actions, and key → pty bytes |
| `src/layout.rs` | pane geometry |
| `src/ui.rs` | sidebar of windows and panes, embedded terminals |
| `src/picker.rs` | fuzzy matching for the cluster picker |
| `src/probe.rs` | reachability probes for the API servers |
| `src/fleet.rs` | clusters, instances, display names |
| `src/kubeconfig.rs` | which files to read, merging, cluster-per-file |
| `src/config.rs` | the tmux-style config parser |

```sh
make          # list targets
make ci       # fmt-check, lint, test — what CI runs
make test     # 286 tests
make run      # run against your kubeconfig
make install  # into ~/.cargo/bin
```

Tests run real child processes on real ptys, using a stub in place of k9s, so
the whole stack — key encoding, pty, terminal emulation, rendering — is covered
without needing a cluster or a k9s binary. CI runs the same `make ci` on Linux
and macOS, since the pty layer is the part most likely to differ.

## Not done yet

- No session persistence: closing k9mux kills its k9s processes.
- Health probes are a TCP connect to the API server — reachability, not auth.
- No scrollback beyond what k9s itself draws.

## License

Apache License 2.0 — see [LICENSE](LICENSE).

Copyright 2026 The k9mux Authors.
