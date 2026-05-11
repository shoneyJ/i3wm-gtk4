# Build & Development

## Reference Project

For Rust + GTK4 syntax reference:

```bash
ls ~/projects/rust-gtk4-todo-app/
```

## Docker Dev Environment

```bash
# Build the container (first time only)
docker compose build

# Enter dev shell
docker compose run --rm dev

# Inside the container
cargo build --release
```

## Copy Release Binary to Host

```bash
# Inside the container, after cargo build --release
cp target/release/i3more /app/dist/
```

The `./dist` directory is bind-mounted from the host, so the binary is immediately available at `./dist/i3more`.

## Test the Binary

```bash
# On the host
./dist/i3more
```

## Building the forked i3 (`vendor/i3`)

The `vendor/i3` submodule is i3 wm forked at `git@github.com:shoneyJ/i3.git`.
It is built only when the dynamic-window-management plan
(`docs/plan/dynamicWM.md`) is on Approach 2 (fork i3).

All build / install / uninstall steps are wrapped in the top-level
`justfile`. Stock i3 at `/usr/bin/i3` is **never** touched — the patched
binary is installed alongside at `/usr/local/bin/i3`, and `just i3-uninstall`
is the no-loss fallback path that takes you back to stock.

### One-time setup

```bash
git submodule update --init vendor/i3
just i3-image                   # build the i3-build container image
```

### Iterative workflow

```bash
just i3-build                   # compile patched i3 → vendor/i3/build/i3
just i3-stage                   # meson install → vendor/i3/build/install-root
just i3-install                 # sudo cp staged tree → /usr/local (prompts for password)
just i3-restart                 # i3-msg restart, swaps the running session in place

# or, the same four steps in one go:
just i3-deploy
```

`which i3` will resolve to `/usr/local/bin/i3` after install because
`/usr/local/bin` precedes `/usr/bin` on `PATH`. `just i3-status` prints the
PATH resolution and the version the running i3 reports.

### Fallback to stock i3

```bash
just i3-uninstall               # removes only /usr/local/bin/i3*
just i3-restart                 # i3-msg restart now picks up /usr/bin/i3
```

If the patched i3 crashes hard and the WM is gone, switch to a TTY
(Ctrl+Alt+F2), log in, then either `just i3-uninstall` (so the next session
boots stock) or `startx /usr/bin/i3`.

### Sync with upstream

```bash
cd vendor/i3
git remote add upstream https://github.com/i3/i3.git    # one-time
git fetch upstream
git rebase upstream/next
cd ../..
git add vendor/i3 && git commit -m "vendor/i3: bump upstream"
```

### Under the hood

`just i3-build` is `docker compose run --rm i3-build bash -c "meson setup
... && ninja -C ..."`; `just i3-stage` adds `--destdir=...`; `just i3-install`
is a single `sudo cp -a`. See `justfile` if you need to override anything.
