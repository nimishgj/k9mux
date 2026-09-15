# Usage

k9mux runs several real `k9s` processes side by side, one per cluster, and gives
you tmux-style windows and panes to arrange them. Nothing inside a pane is
k9mux — it is k9s, the same binary and version you already have.

- [Starting up](#starting-up)
- [Windows and panes](#windows-and-panes)
- [Moving between clusters](#moving-between-clusters)
- [The sidebar](#the-sidebar)
- [When k9s quits or crashes](#when-k9s-quits-or-crashes)
- [Key handling](#key-handling)
- [Troubleshooting](#troubleshooting)

## Starting up

```sh
k9mux                                       # $KUBECONFIG, or ~/.kube/config
k9mux --kubeconfig ~/.kube/config-prod    # one specific file
KUBECONFIG=~/.kube/a:~/.kube/b k9mux        # several, merged
k9mux --keys                                # print the default bindings
k9mux --help
```

`k9s` must be on your `PATH`.

k9mux opens one window named `k9mux`, containing one pane, running k9s against
the first cluster it found. From there you add panes for other clusters and
more windows for other tasks.

## Windows and panes

A **pane** is one k9s process pointed at one cluster. A **window** is a group
of panes filling the screen. This mirrors tmux, and the default keys do too.

All of these are typed after the prefix, `option-space` by default, written
`⌥space` below. Press the prefix, let go, then press the command key.

| | |
|---|---|
| `⌥space` `c` | new window |
| `⌥space` `,` | rename the current window |
| `⌥space` `n` / `p` | next / previous window |
| `⌥space` `1`…`4` | jump to a window by number |
| `⌥space` `%` | split the window into side-by-side panes |
| `⌥space` `"` | split the window into stacked panes |
| `⌥space` `x` | close the focused pane |
| `⌥space` `r` | restart k9s in the focused pane |

Moving between panes needs no prefix:

| | |
|---|---|
| `⌥h` `⌥j` `⌥k` `⌥l` | move focus left / down / up / right |
| `⌥space` `Tab` | focus the next pane |

A split divides **the focused pane**, not the whole window — the same as tmux.
Split once with `%` and you get two panes side by side; focus the right one and
press `"` and only that pane is stacked, leaving the left one full height:

```
┌──────────┬──────────┐
│          │    2     │
│    1     ├──────────┤
│          │    3     │
└──────────┴──────────┘
```

Closing a pane gives its space back to the pane it was split from. A split is
refused if the halves would be too small to be useful, so there is no fixed
pane limit — just how much room you have.

Focus movement is geometric: `⌥h` from pane 3 above goes to pane 1, because
that is what is actually to its left. Focus stops at the edges rather than
wrapping.

Each window keeps its own panes, layout and focused pane, so switching windows
puts everything back exactly as you left it.

Renaming opens an inline prompt in the sidebar, pre-filled with the current
name. `Enter` commits, `Esc` cancels, and an empty name keeps the old one.
While you are typing a name, keys go to the prompt rather than to k9s.

## Moving between clusters

Each pane is pointed at one cluster. To change which:

| | |
|---|---|
| `⌥space` `>` | next cluster in the focused pane |
| `⌥space` `<` | previous cluster |
| click a pane in the sidebar | focus it |

Switching a pane's cluster restarts k9s in that pane against the new context.
A new pane or window picks the first cluster that is not already open
somewhere, so splitting repeatedly walks you through the fleet.

## The sidebar

The sidebar lists your windows, with the current window's panes underneath it:

```
k9mux  2 windows

1:edge
2:edge*
  ● apps-eks
  ● data-eks
```

`*` marks the current window. The dot shows what k9mux knows about each
cluster: green once k9s started against it, red if the process could not be
started at all, grey before it has been tried.

Click a window to switch to it, or a pane to focus it. The key hints along the
bottom are generated from your own bindings, so they always match your config.

`⌥space` `e` collapses the sidebar and gives the full width to k9s; press it
again to bring it back.

## When k9s quits or crashes

Quitting k9s inside a pane with `:q` closes that pane, and the remaining panes
expand to fill the space. Closing a window's last pane closes the window, and
closing the last window quits k9mux. This is the tmux model: the pane exists to
hold the process, so when the process ends the pane goes with it.

A k9s that exits *non-zero* is treated differently — the pane stays, showing

```
k9s exited (exit 1) — ⌥space r to restart
```

so whatever went wrong stays readable. `⌥space r` restarts it.

If a cluster is unreachable, k9s will sit there retrying rather than exiting;
`⌥space x` closes the pane.

## Key handling

Every key that is not a k9mux binding goes straight to the k9s in the focused
pane, encoded as a terminal would send it. That includes `:`, `/`, `ctrl-a`,
`|`, digits, function keys and arrows — none of k9s' own keys are shadowed.

To send the prefix itself to k9s, press it twice.

Option keys are accepted in both forms. If your terminal sends option as meta,
they arrive as a real `alt` modifier. If not — the macOS default — they arrive
as glyphs (`˙` for `⌥h`, `¬` for `⌥l`, a non-breaking space for `⌥space`), and
k9mux maps those back. Either way the bindings work, with no terminal setup.

## Troubleshooting

**`is k9s on your PATH?`** — the pane could not start the binary. Install k9s,
or restart the pane with `⌥space r` once it is installed.

**`no contexts in …`** — k9mux could not find a kubeconfig. It prints the paths
it tried. See [configuration](configuration.md#kubeconfig) for the search
order, and pass `--kubeconfig` to be explicit.

**A pane is blank** — k9s is probably still connecting. Against an unreachable
cluster it may stay that way; `⌥space x` closes the pane.

**The prefix does nothing** — some terminals intercept `option-space`. Rebind
it to something free:

```sh
set prefix C-b
```

**Panes are cramped** — hide the sidebar with `⌥space e`, or use fewer panes
per window and more windows.
