# Configuration

k9mux reads `~/.config/k9mux/config`, one directive per line, in the style of
`tmux.conf`. Everything in it is optional; without a config file k9mux uses the
defaults below and whatever kubeconfig your shell already points at.

`config.example` in the repo is a copyable starting point, and `k9mux --keys`
prints the current defaults in exactly this syntax.

- [Syntax](#syntax)
- [Changing the prefix](#changing-the-prefix)
- [Binding keys](#binding-keys)
- [Key names](#key-names)
- [Commands](#commands)
- [Kubeconfig](#kubeconfig)
- [Grouping contexts into clusters](#grouping-contexts-into-clusters)
- [Default bindings](#default-bindings)
- [Worked examples](#worked-examples)

## Syntax

One directive per line. `#` starts a comment. Words are separated by spaces,
and single or double quotes hold a word together — needed for `"` itself, and
handy for `<` and `>`.

```sh
# a comment
bind '"' split-horizontal     # and a trailing one
cluster 'prod us' use1-a use1-b
```

An error names the line and what was wrong, and k9mux refuses to start rather
than silently ignoring it:

```
k9mux: /Users/you/.config/k9mux/config: line 4: unknown command "splt-vertical"
```

## Changing the prefix

```sh
set prefix C-b        # tmux's prefix
set prefix M-space    # the default
set prefix F1
```

Only one prefix key at a time. To send it through to k9s instead of using it
as a prefix, press it twice.

## Binding keys

```sh
bind <key> <command>        # after the prefix
bind -n <key> <command>     # no prefix, a direct chord
unbind <key>                # hand the key back to k9s
unbind -n <key>
```

`bind` replaces whatever that key did, so rebinding a default just works.
`unbind` removes it entirely and the key is forwarded to k9s like any other.

Bindings are resolved in this order: the prefix key first, then `-n` root
bindings, then everything else goes to k9s. That means binding the prefix key
itself with `-n` has no effect — the prefix wins.

## Key names

A key is one of:

- a single character — `x`, `%`, `?`, `'"'`
- a modifier form — `C-a` (control), `M-x` (option/meta), `C-M-x` (both)
- a name, case-insensitive:

  `Space` `Tab` `BTab` `Enter` `Escape` `BSpace` `Delete` `Insert`
  `Home` `End` `PageUp` `PageDown` `Up` `Down` `Left` `Right` `F1`–`F12`

Shift is not written separately — the character already carries it, so `%` is
just `%`.

`M-` bindings also match the glyphs macOS sends when the option key is not
configured as meta, so `bind -n M-h focus-left` works whether your terminal
sends `alt`+`h` or `˙`.

## Commands

**Windows**

| command | |
|---|---|
| `new-window` | open a window on the next unused cluster |
| `rename-window` | rename the current window |
| `next-window` / `prev-window` | cycle windows |
| `select-window-0` … `select-window-9` | jump to a window by index (0-based) |

**Panes**

| command | |
|---|---|
| `split-vertical` | split the focused pane side by side |
| `split-horizontal` | split the focused pane top and bottom |
| `close-pane` | close the focused pane |
| `focus-left` `focus-right` `focus-up` `focus-down` | move focus geometrically |
| `focus-next` | cycle to the next pane |
| `restart` | restart k9s in the focused pane |

**Clusters**

| command | |
|---|---|
| `next-cluster` / `prev-cluster` | repoint the focused pane at another cluster |

**Other**

| command | |
|---|---|
| `toggle-sidebar` | collapse or show the sidebar |
| `help` | the help overlay |
| `quit` | quit k9mux |

## Kubeconfig

k9mux does not talk to your clusters; it passes a kubeconfig and a `--context`
down to each k9s. Files are found in this order, first match wins:

1. `--kubeconfig` on the command line (colon separated)
2. `$KUBECONFIG` (colon separated)
3. `kubeconfig` lines in the config file
4. `~/.kube/config`

```sh
kubeconfig ~/.kube/config-prod
kubeconfig ~/.kube/config-remote
```

Several files are merged the way kubectl merges them: a context defined in two
files belongs to the first file that had it. `~` is expanded.

Context names are shortened for display — EKS ARNs to the cluster name, GKE
paths to the last segment, DNS names to their first label. If shortening would
make two contexts look alike, both keep their full names. k9s is always
launched with the real context name.

## Grouping contexts into clusters

```sh
cluster prod-us use1-a use1-b usw2-a
cluster prod-eu euc1-a euc1-b
```

Without any `cluster` lines, **each kubeconfig file becomes one cluster**,
named after the file: `~/.kube/config-prod` shows up as `prod`, and a
plain `~/.kube/config` as `default`. Contexts you list nowhere end up in a
group called `contexts`.

Grouping decides the order panes walk through with `next-cluster`, and which
cluster a new pane or window picks up.

## Default bindings

```sh
set prefix M-space

bind c   new-window
bind ,   rename-window
bind n   next-window
bind p   prev-window
bind 1   select-window-0
bind 2   select-window-1
bind 3   select-window-2
bind 4   select-window-3

bind '"' split-horizontal
bind %   split-vertical
bind x   close-pane
bind Tab focus-next
bind '<' prev-cluster
bind '>' next-cluster
bind r   restart
bind e   toggle-sidebar
bind ?   help
bind q   quit

bind -n M-h focus-left
bind -n M-j focus-down
bind -n M-k focus-up
bind -n M-l focus-right
```

## Worked examples

**Use tmux's prefix**

```sh
set prefix C-b
```

**Move pane focus with control-arrows instead of option-hjkl**

```sh
unbind -n M-h
unbind -n M-j
unbind -n M-k
unbind -n M-l

bind -n C-Left  focus-left
bind -n C-Down  focus-down
bind -n C-Up    focus-up
bind -n C-Right focus-right
```

**Split with `|` and `-` instead**

```sh
bind '|' split-vertical
bind -   split-horizontal
```

**Give a key back to k9s**

```sh
unbind e        # e now reaches k9s; the sidebar toggle is gone
```

**A full config**

```sh
set prefix C-b

bind '|' split-vertical
bind -   split-horizontal
bind -n C-Left  focus-left
bind -n C-Right focus-right

kubeconfig ~/.kube/config-prod
kubeconfig ~/.kube/config-remote

cluster edge  edge-k8s
cluster eks      apps-eks apps-eks
cluster local    k3s microk8s
```
