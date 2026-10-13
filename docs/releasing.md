# Building and releasing

`cargo run` is for working on k9mux. What you install and what you publish are
built differently: optimised, stripped, and for a named platform.

```sh
make build      # an optimised binary for this machine, at target/release/k9mux
make install    # the same, into ~/.cargo/bin
make dist       # release tarballs, one per architecture of this OS
```

## A release is a tag

```sh
# Cargo.toml: version = "0.2.0"
cargo build                                  # refreshes Cargo.lock
git commit -am "Release 0.2.0"
git tag -a v0.2.0 -m v0.2.0
git push origin main v0.2.0
```

Pushing the tag is the whole release. The
[release workflow](../.github/workflows/release.yml) builds all four platforms
in parallel and, for each one:

1. builds `--release --locked`, so the published binary is built from the
   committed `Cargo.lock` and nothing resolves differently on the day;
2. runs the test suite, wherever the runner can run what it just built;
3. packages the binary with the README, `docs/` and `config.example` into
   `k9mux-<version>-<target>.tar.gz`, with a `.sha256` beside it.

Then it creates the GitHub release with generated notes and all eight files
attached. The publish step refuses to run if the tag and the version in
`Cargo.toml` disagree, so a mistyped tag fails instead of shipping a tarball
whose name does not match what is in it.

Running the workflow by hand (**Actions → release → Run workflow**) builds and
packages everything the same way but publishes nothing, keeping the tarballs as
run artifacts. That is the way to check a release will build before tagging.

## What is built where

| target | runner | |
|---|---|---|
| `aarch64-apple-darwin` | `macos-latest` | native, tested |
| `x86_64-apple-darwin` | `macos-latest` | cross-compiled |
| `aarch64-unknown-linux-musl` | `ubuntu-24.04-arm` | native, tested |
| `x86_64-unknown-linux-musl` | `ubuntu-latest` | native, tested |

Each architecture is built on its own architecture wherever a runner exists for
it, so the tests run against the binary that actually ships. The only exception
is the Intel mac build, which is cross-compiled on an Apple silicon runner:
GitHub's hosted macOS runners are all Apple silicon now. It is built from the
same source and the same lockfile, but nothing executes it before release.

Linux is built against **musl**, statically linked: one binary per
architecture that runs on any distribution, old or new, with no glibc version
to match. It is how k9s itself ships, and it means a k9mux tarball works on a
container image as readily as on a laptop.

## Building for another platform locally

`make dist` builds what your own machine can build natively — both macOS
architectures on a Mac, since Apple's toolchain cross-compiles between them
without any extra setup.

Linux tarballs need a Linux toolchain, so they are built in a container:

```sh
make dist-linux     # both Linux architectures, via docker
make dist-all       # this OS and Linux, i.e. everything a release ships
```

The non-native architecture runs under emulation and is slow — fine for a
one-off, which is why the release itself is built on real runners of each
architecture. Override `DOCKER=podman` or `LINUX_IMAGE=...` if you want
something other than `docker` and `rust:1-alpine`.

Everything lands in `dist/`:

```
dist/k9mux-0.1.0-aarch64-apple-darwin.tar.gz
dist/k9mux-0.1.0-aarch64-apple-darwin.tar.gz.sha256
```

Both the Makefile and the workflow go through
[`scripts/dist.sh`](../scripts/dist.sh), so a tarball you build on your laptop
is put together exactly like a published one.

## Homebrew

`brew install nimishgj/tap/k9mux` is served by a *tap*: the repository
[nimishgj/homebrew-tap](https://github.com/nimishgj/homebrew-tap), holding the
formula at `Formula/k9mux.rb`. It already exists; recreating one from scratch
is a single command:

```sh
gh repo create nimishgj/homebrew-tap --public \
  -d "Homebrew formulae for k9mux"
```

Both this repository and the tap have to be **public**. Homebrew fetches
release assets anonymously, so a private asset answers with a 404 no matter
who is asking.

After that, each release can update the formula on its own. Create a
fine-grained token with `contents: write` on the tap repository and add it
here as the secret `HOMEBREW_TAP_TOKEN`; the `tap` job then commits a fresh
formula on every tag. Without the secret the job says so and succeeds, so a
release never fails for want of it.

By hand it is two commands:

```sh
gh release download v0.1.0 --pattern '*.sha256' -D dist
scripts/brew-formula.sh 0.1.0 > ../homebrew-tap/Formula/k9mux.rb
```

[`scripts/brew-formula.sh`](../scripts/brew-formula.sh) reads the checksums
out of the `.sha256` files in `dist/`, so the formula can only ever claim a
hash that something really produced — it cannot drift from the release the
way a hand-edited formula does. `BASE_URL` points it elsewhere, which is how
you test a formula against local tarballs before publishing anything:

```sh
make dist
BASE_URL="file://$PWD/dist" scripts/brew-formula.sh 0.1.0
```

The formula declares `depends_on "k9s"`, from homebrew-core, since a pane
with no k9s to run is of no use. On a machine where k9s came from somewhere
else — another tap, asdf, a plain download — Homebrew will install its own
copy alongside.

Submitting k9mux to homebrew-core instead of a tap is not worth trying yet:
core wants a project with real usage behind it, and refuses anything that
cannot show it.

## Notes

- **The binaries are not signed or notarised.** A tarball downloaded by a
  browser on macOS is quarantined and Gatekeeper will refuse to run what came
  out of it; `xattr -dr com.apple.quarantine k9mux` clears it. Anything fetched
  with `curl` is not quarantined in the first place. Signing would need an
  Apple Developer certificate in the repository secrets.
- **`ubuntu-24.04-arm` is free for public repositories.** If this repo ever
  goes private, that runner needs a plan that includes arm64 runners — or the
  arm64 Linux build has to be cross-compiled on an x86 runner instead.
- **The release profile uses thin LTO and strips symbols**, which roughly
  halves the binary at the cost of a slower build. Release builds are rare
  enough for that to be the right trade.
