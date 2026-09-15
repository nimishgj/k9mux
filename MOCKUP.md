# mk9s — multi-cluster k9s TUI

Mockups for a k9s-style terminal UI where the unit of navigation is a **fleet**:
`cluster group -> N instances`. Every resource view can be scoped to one instance,
one cluster, or the whole fleet, and every action can fan out across the scope.

Legend: `●` healthy  `◐` degraded  `○` unreachable  `✗` error  `◇` pinned  `⟳` refresh interval

---

## 1. Fleet view — aggregated resources

Default screen. Left rail is the fleet tree, right is the resource table with an
`INSTANCE` column injected. Rows from unreachable instances are never silently
dropped — the rail shows what is missing from the result set.

```
┌─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ mk9s  ⎈ 0.1.0        scope ALL · 3 clusters · 9 instances        ns <all>       ⟳ 2s  ● live                            │
│ <pods>   /filter                                            [1/4] api-latency p95 48ms                                  │
├────────────────────────┬────────────────────────────────────────────────────────────────────────────────────────────────┤
│ FLEET            8/9 ● │ PODS  1-18 of 312                                   agg: 9 instances · 1 stale                 │
│ ────────────────────── │ ────────────────────────────────────────────────────────────────────────────────────────────── │
│ ▾ prod-us    4/4 ●     │ INSTANCE     NAMESPACE   NAME                           READY  STATUS     RS  CPU   MEM    AGE │
│   ● use1-a      112p   │ use1-a       payments    checkout-7d9f4b6c88-4mzql      2/2    Running     0  142m  310Mi   4d │
│   ● use1-b       98p   │ use1-b       payments    checkout-7d9f4b6c88-q7t2v      2/2    Running     0  138m  298Mi   4d │
│   ● usw2-a       74p   │ usw2-a       payments    checkout-7d9f4b6c88-x1n8k      2/2    Running     1  151m  333Mi   4d │
│   ● usw2-b       81p   │ usw2-b       payments    checkout-7d9f4b6c88-bb04r      1/2    CrashLoop   7   12m   88Mi  11m │
│ ▾ prod-eu    3/4 ◐     │ euc1-a       payments    checkout-59c4d7f6b9-lm3wq      2/2    Running     0  128m  281Mi   2d │
│   ● euc1-a       66p   │ euc1-b       payments    checkout-59c4d7f6b9-zp9hd      2/2    Running     0  133m  290Mi   2d │
│   ◐ euc1-b       61p   │ euw2-a       payments    checkout-59c4d7f6b9-t5vcx      0/2    Pending     0     —      —  38s │
│   ● euw2-a       58p   │ euw2-b       ✗ unreachable — last sync 3m12s ago · dial tcp i/o timeout           [r] retry    │
│   ○ euw2-b        ——   │ use1-a       identity    session-api-6b8c9d-2wkfp       1/1    Running     0   44m  120Mi  16d │
│ ▾ staging    2/2 ●     │ use1-b       identity    session-api-6b8c9d-hhq7s       1/1    Running     0   41m  118Mi  16d │
│   ● stg-blue     37p   │ usw2-a       identity    session-api-6b8c9d-c8dlp       1/1    Running     0   39m  115Mi  16d │
│   ● stg-green    34p   │ euc1-a       identity    session-api-77ffab-nn2bq       1/1    Running     0   37m  109Mi   9d │
│                        │ stg-blue     identity    session-api-77ffab-4dd6t       1/1    Running     2   35m  104Mi   9d │
│ ◇ pinned               │ stg-green    ledger      ledger-worker-5f7b2c-9jjvm     1/1    Running     0   88m  412Mi   3d │
│   ◇ usw2-b  ✗ 1 pod    │ use1-a       ledger      ledger-worker-5f7b2c-mm81w     1/1    Running     0   91m  428Mi   3d │
│   ◇ euw2-b  ○ down     │ use1-b       ledger      ledger-worker-5f7b2c-r4k2d     1/1    Running     0   87m  404Mi   3d │
│                        │ usw2-a       ledger      ledger-worker-5f7b2c-vv7xn     1/1    Running     0   93m  441Mi   3d │
├────────────────────────┴────────────────────────────────────────────────────────────────────────────────────────────────┤
│ <0> all  <1> prod-us  <2> prod-eu  <3> staging   ⇥ cycle instance   g group-by   , compare   .                          │
│ ↑↓ move  ⏎ drill  d describe  l logs  s shell  ⌫ back  : cmd  ? help                  fan-out: ⌥⏎                       │
└─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```


## 2. `:fleet` — cluster & instance manager

Where instances are registered, grouped and health-checked. An instance is a
kubeconfig context plus fleet metadata (group, region, tier, colour, tags).

```
┌─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ mk9s  ⎈ 0.1.0                      :fleet                              ⟳ 10s  ● 8/9 reachable                           │
├─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│  NAME        GROUP      REGION      VER      NODES  PODS   CPU%   MEM%   LAT    STATUS                                  │
│  ──────────────────────────────────────────────────────────────────────────────────────────────────────────             │
│▸ use1-a      prod-us    us-east-1   v1.31.4     18    112   61%    72%   14ms   ● healthy                               │
│  use1-b      prod-us    us-east-1   v1.31.4     18     98   57%    69%   16ms   ● healthy                               │
│  usw2-a      prod-us    us-west-2   v1.31.4     12     74   44%    58%   62ms   ● healthy                               │
│  usw2-b ◇    prod-us    us-west-2   v1.31.2     12     81   49%    63%   65ms   ● healthy  ⚠ skew                       │
│  euc1-a      prod-eu    eu-central  v1.30.9     10     66   38%    51%  104ms   ● healthy                               │
│  euc1-b      prod-eu    eu-central  v1.30.9     10     61   88%    91%  131ms   ◐ pressure                              │
│  euw2-a      prod-eu    eu-west-2   v1.30.9      8     58   35%    47%  118ms   ● healthy                               │
│  euw2-b ◇    prod-eu    eu-west-2   ——           —      —     —      —      —   ○ unreachable                           │
│  stg-blue    staging    us-east-1   v1.32.0      4     37   22%    34%   15ms   ● healthy                               │
│  stg-green   staging    us-east-1   v1.32.0      4     34   20%    31%   15ms   ● healthy                               │
│                                                                                                                         │
│  ╭─ euw2-b ─────────────────────────────────────────────────────────────────────────╮                                   │
│  │ context   arn:aws:eks:eu-west-2:41••••:cluster/euw2-b                            │                                   │
│  │ tags      tier=prod  wave=2  owner=platform  colour=#e5534b                      │                                   │
│  │ error     Get "https://…/version": dial tcp 10.42.7.9:443: i/o timeout           │                                   │
│  │ retries   4 (backoff 30s)   last ok 14:02:51   auto-exclude from fan-out: ON     │                                   │
│  ╰──────────────────────────────────────────────────────────────────────────────────╯                                   │
├─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ a add  e edit  p pin  x exclude  r reconnect  G regroup  ⏎ enter instance  ⌫ back                                       │
└─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```


## 3. `,` — compare mode (same workload across instances)

Drift detection. Pick a workload, see it side by side across every instance in
scope; differing cells are called out so config skew is obvious at a glance.

```
┌─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ mk9s   compare · deploy/checkout · ns payments · scope prod-us,prod-eu    7 instances  ⟳ 5s                             │
├─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│                    use1-a    use1-b    usw2-a    usw2-b    euc1-a    euc1-b    euw2-a                                   │
│  ──────────────────────────────────────────────────────────────────────────────────────────────────────────             │
│  image tag         v2.14.1   v2.14.1   v2.14.1   v2.13.0   v2.14.1   v2.14.1   v2.14.1     ✗                            │
│  replicas          6         6         4         4         4         4         2           ✗                            │
│  ready             6/6       6/6       4/4       1/4       4/4       4/4       0/2         ✗                            │
│  cpu request       250m      250m      250m      250m      250m      250m      250m        ✓                            │
│  mem limit         512Mi     512Mi     512Mi     512Mi     512Mi     512Mi     512Mi       ✓                            │
│  cfg checksum      a91f      a91f      a91f      7c02      a91f      a91f      a91f        ✗                            │
│  secret rev        14        14        14        14        14        13        14          ✗                            │
│  hpa min/max       4/12      4/12      4/12      4/12      4/12      4/12      2/6         ✗                            │
│  restarts 1h       0         0         1         7         0         0         0           ✗                            │
│  p95 latency       48ms      51ms      63ms      ——        104ms     129ms     ——          ⚠                            │
│  rollout age       4d2h      4d2h      4d2h      11m       2d6h      2d6h      38s         ⚠                            │
│                                                                                                                         │
│  ╭─ drift ──────────────────────────────────────────────────────────────────────────╮                                   │
│  │ 4 of 7 fields differ. Outliers:                                                  │                                   │
│  │   usw2-b   image v2.13.0 (behind 1 release) · cfg checksum 7c02 · 7 restarts     │                                   │
│  │   euc1-b   secret rev 13 (behind 1)                                              │                                   │
│  │   euw2-a   replicas 2, hpa 2/6 — scaled differently from the rest of the fleet   │                                   │
│  │ [y] copy usw2-b spec from use1-a    [D] full yaml diff    [R] rollout to match   │                                   │
│  ╰──────────────────────────────────────────────────────────────────────────────────╯                                   │
├─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ ←→ column  ↑↓ field  D yaml diff  y align  f only-diffs  ⌫ back                                                         │
└─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```


## 4. `l` — multiplexed logs across instances

One stream, interleaved by timestamp, each line prefixed with its instance in that
instance's colour. Per-instance mute, plus a merge/stack toggle for tailing.

```
┌─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ mk9s   logs · deploy/checkout · payments · 7 instances · merged        ⏸ follow ON  ⟳ live                              │
│ /filter: level=error                                            12,481 lines · 38 matched                               │
├────────────────────────┬────────────────────────────────────────────────────────────────────────────────────────────────┤
│ STREAMS                │ 14:07:02.114  use1-a  │ INFO  order 8812 authorized in 41ms                                    │
│ ────────────────────── │ 14:07:02.119  euc1-a  │ INFO  order 8813 authorized in 96ms                                    │
│ ● use1-a    4.1k  ▂▃▂▃ │ 14:07:02.204  usw2-b  │ ERROR failed to load cfg: checksum 7c02 mismatch                       │
│ ● use1-b    3.9k  ▂▂▃▂ │ 14:07:02.204  usw2-b  │ ERROR   at cfg.Load (config.go:88)                                     │
│ ● usw2-a    2.8k  ▂▃▅▃ │ 14:07:02.311  use1-b  │ INFO  order 8814 authorized in 44ms                                    │
│ ✗ usw2-b      44  ▇▇▇▇ │ 14:07:02.402  euc1-b  │ WARN  memory pressure 91%, shedding load                               │
│ ● euc1-a    1.2k  ▂▂▂▃ │ 14:07:03.001  usw2-b  │ FATAL exit status 1 — restarting (7)                                   │
│ ◐ euc1-b      88  ▃▅▇▅ │ 14:07:03.118  use1-a  │ INFO  order 8815 authorized in 39ms                                    │
│ ○ euw2-a      ——  ──── │ 14:07:03.220  usw2-a  │ INFO  order 8816 authorized in 58ms                                    │
│ ⊘ euw2-b   muted       │ 14:07:03.417  euc1-b  │ WARN  memory pressure 92%, shedding load                               │
│                        │ 14:07:04.006  usw2-b  │ ERROR failed to load cfg: checksum 7c02 mismatch                       │
│ m mute   M solo        │ 14:07:04.112  use1-b  │ INFO  order 8817 authorized in 43ms                                    │
│ s stack  w wrap        │ 14:07:04.298  euc1-a  │ INFO  order 8818 authorized in 91ms                                    │
│ t timestamps           │ ─────────────────────────────────────────────────────── ⤓ live ──────────────────              │
├────────────────────────┴────────────────────────────────────────────────────────────────────────────────────────────────┤
│ ⌥m mute instance  p prev container  c clear  S save  / filter  ⇧/ regex  ⌫ back                                         │
└─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```


## 5. `⌥⏎` — fan-out action with blast-radius guard

Any mutating action asks first, spells out exactly which instances it will touch,
and runs in waves. Prod fan-out requires typing the scope name to confirm.

```
┌─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ mk9s   scope prod-us · 4 instances                                           ⟳ paused  ⏸                                │
├─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│        ╭─ FAN-OUT ─────────────────────────────────────────────────────────────╮                                        │
│        │  rollout restart  deploy/checkout  -n payments                        │                                        │
│        │                                                                       │                                        │
│        │  blast radius   4 instances · 20 pods · ~3.1k req/s                   │                                        │
│        │  strategy       waves of 1, 30s soak, halt on error                   │                                        │
│        │                                                                       │                                        │
│        │    ✓ use1-a     6 pods    wave 1                                      │                                        │
│        │    ✓ use1-b     6 pods    wave 2                                      │                                        │
│        │    ✓ usw2-a     4 pods    wave 3                                      │                                        │
│        │    ✓ usw2-b     4 pods    wave 4    ⚠ already crashlooping            │                                        │
│        │    ⊘ euw2-b     excluded — unreachable                                │                                        │
│        │                                                                       │                                        │
│        │  ⚠ this scope is tagged tier=prod                                     │                                        │
│        │  type the scope name to confirm:  prod-us▌                            │                                        │
│        │                                                                       │                                        │
│        │  ␣ toggle instance   ⇧w wave size   d dry-run   ⏎ run   esc cancel    │                                        │
│        ╰───────────────────────────────────────────────────────────────────────╯                                        │
│                                                                                                                         │
│        ╭─ running ─────────────────────────────────────────────────────────────╮                                        │
│        │  wave 1  use1-a   ████████████████████  done   6/6 ready      42s     │                                        │
│        │  wave 2  use1-b   ███████████░░░░░░░░░  40%    2/6 ready      18s     │                                        │
│        │  wave 3  usw2-a   ░░░░░░░░░░░░░░░░░░░░  queued                        │                                        │
│        │  wave 4  usw2-b   ░░░░░░░░░░░░░░░░░░░░  queued                        │                                        │
│        │                                            [h] halt   [s] skip wave   │                                        │
│        ╰───────────────────────────────────────────────────────────────────────╯                                        │
└─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```


## 6. `:pulse` — fleet health grid

The ambient screen you leave open on a second monitor. One card per instance,
sorted by worst-first so trouble floats to the top-left.

```
┌─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ mk9s   :pulse · 3 clusters · 10 instances                        ⟳ 5s   worst-first   ● live                            │
├─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│  ╭─ euw2-b  ○ ──────────╮ ╭─ usw2-b  ✗ ──────────╮ ╭─ euc1-b  ◐ ──────────╮                                             │
│  │ prod-eu / eu-west-2  │ │ prod-us / us-west-2  │ │ prod-eu / eu-central │                                             │
│  │ unreachable   3m12s  │ │ 1 crashloop          │ │ mem 91%  cpu 88%     │                                             │
│  │                      │ │ checkout  7 restarts │ │ node pressure x2     │                                             │
│  │ cpu   ────────────   │ │ cpu   ▃▃▅▇▇▅▃▂▂▃▅▇   │ │ cpu   ▅▅▇▇▇▇▇▅▇▇▇▇   │                                             │
│  │ mem   ────────────   │ │ mem   ▂▃▃▅▅▇▇▇▅▃▂▂   │ │ mem   ▇▇▇▇▇▇▇▇▇▇▇▇   │                                             │
│  │ pods  ——             │ │ pods  81  ready 77   │ │ pods  61  ready 61   │                                             │
│  ╰──────────────────────╯ ╰──────────────────────╯ ╰──────────────────────╯                                             │
│  ╭─ use1-a  ● ──────────╮ ╭─ use1-b  ● ──────────╮ ╭─ usw2-a  ● ──────────╮                                             │
│  │ prod-us / us-east-1  │ │ prod-us / us-east-1  │ │ prod-us / us-west-2  │                                             │
│  │ all systems normal   │ │ all systems normal   │ │ all systems normal   │                                             │
│  │                      │ │                      │ │                      │                                             │
│  │ cpu   ▂▃▃▂▃▃▂▂▃▃▂▃   │ │ cpu   ▂▂▃▃▂▂▃▂▂▃▃▂   │ │ cpu   ▂▃▂▂▃▃▅▃▂▂▃▂   │                                             │
│  │ mem   ▅▅▅▅▅▅▅▅▅▅▅▅   │ │ mem   ▅▅▅▅▅▅▅▅▅▅▅▅   │ │ mem   ▃▃▅▅▅▃▃▅▅▅▃▃   │                                             │
│  │ pods  112 ready 112  │ │ pods  98  ready 98   │ │ pods  74  ready 74   │                                             │
│  ╰──────────────────────╯ ╰──────────────────────╯ ╰──────────────────────╯                                             │
│  ╭─ euc1-a  ● ──────────╮ ╭─ euw2-a  ● ──────────╮ ╭─ stg-blue ● ─────────╮                                             │
│  │ prod-eu / eu-central │ │ prod-eu / eu-west-2  │ │ staging / us-east-1  │                                             │
│  │ all systems normal   │ │ 1 pod pending  38s   │ │ all systems normal   │                                             │
│  │                      │ │                      │ │                      │                                             │
│  │ cpu   ▂▂▃▂▂▃▂▂▂▃▂▂   │ │ cpu   ▂▂▂▃▂▂▂▂▃▂▂▂   │ │ cpu   ▁▂▂▁▂▂▁▁▂▂▁▂   │                                             │
│  │ mem   ▃▃▃▅▃▃▃▃▅▃▃▃   │ │ mem   ▃▃▃▃▃▃▃▃▃▃▃▃   │ │ mem   ▂▂▃▂▂▂▃▂▂▂▂▃   │                                             │
│  │ pods  66  ready 66   │ │ pods  58  ready 57   │ │ pods  37  ready 37   │                                             │
│  ╰──────────────────────╯ ╰──────────────────────╯ ╰──────────────────────╯                                             │
├─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ ⏎ enter instance  z zoom  o sort  f filter group  x exclude  ⌫ back                                                     │
└─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```


## 7. `:` — command palette with scope syntax

Scope is part of the command grammar, so any view can be addressed directly:
`:<resource> @<scope> -n <ns>`.

```
┌─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ mk9s   scope ALL                                                                                                        │
├─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│   ╭──────────────────────────────────────────────────────────────────────────────╮                                      │
│   │ : po @prod-eu -n payments▌                                                   │                                      │
│   ╰──────────────────────────────────────────────────────────────────────────────╯                                      │
│     po        @prod-eu       pods in prod-eu (4 instances)                  resource                                    │
│     po        @euc1-a        pods on euc1-a only                            resource                                    │
│     po        @*             pods across all 10 instances                   resource                                    │
│     po        @tier=prod     pods on instances tagged tier=prod (8)         selector                                    │
│     po        @◇             pods on pinned instances (2)                   selector                                    │
│                                                                                                                         │
│     :fleet                   manage clusters and instances                                                              │
│     :pulse                   fleet health grid                                                                          │
│     :drift  deploy/checkout  compare a workload across scope                                                            │
│     :ctx    prod-eu          change active scope                                                                        │
│     :xray   svc @prod-us     dependency tree, per instance                                                              │
│     :alias                   saved scopes and shortcuts                                                                 │
│                                                                                                                         │
│   scope grammar   @<group> · @<instance> · @<k>=<v> · @* · @◇ · comma-separated union                                   │
│   examples        :deploy @prod-us,prod-eu     :po @region=eu-west-2     :no @*                                         │
└─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## Model behind the mockups

| Concept | Meaning |
|---|---|
| **instance** | one reachable Kubernetes API endpoint (a kubeconfig context) + fleet metadata |
| **cluster / group** | a named set of instances (`prod-us` = 4 regional instances) |
| **scope** | the current selection: one instance, one group, a tag selector, or the whole fleet |
| **fan-out** | running a read or a mutation against every instance in scope, in waves |
| **drift** | field-level comparison of the same object across instances in scope |

Design rules the mockups encode:

1. **Scope is always visible** in the header — you can never act without knowing the blast radius.
2. **Partial failure is data, not an error.** Unreachable instances get a row/card, never a silent gap.
3. **Instance identity is colour + short name**, consistent across every view including log prefixes.
4. **Reads fan out by default, writes never do** — writes require explicit `⌥⏎` and a typed confirm on prod.
5. **k9s muscle memory is preserved**; multi-cluster additions live on modifier keys and `:` commands.
