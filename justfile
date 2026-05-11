# i3More — build & install the patched i3 fork (vendor/i3).
#
# Approach 2 of docs/plan/dynamicWM.md: patches live in vendor/i3 (submodule
# pointed at git@github.com:shoneyJ/i3.git). Stock i3 at /usr/bin/i3 is the
# apt-managed fallback and is never touched by any recipe here. `just
# i3-uninstall` removes only the /usr/local copy, leaving /usr/bin/i3 in
# place so `i3-msg restart` (or a fresh login) returns to stock.
#
# Build tree at vendor/i3/build is root-owned because the i3-build container
# runs as root. All clean / rm steps therefore run inside the container — no
# host sudo needed for those. The single host-sudo step is `i3-install`
# itself (cp into /usr/local).

i3_src      := "vendor/i3"
i3_build    := i3_src + "/build"
i3_staged   := i3_build + "/install-root"

# List every recipe with its description
default:
    @just --list

# --- build the i3-build container image -------------------------------------

# (re)build Dockerfile.i3 — run after editing Dockerfile.i3 or DEPENDS
i3-image:
    docker compose build i3-build

# --- compile the patched i3 -------------------------------------------------

# Configure with meson if missing, then ninja-build into vendor/i3/build/
i3-build:
    docker compose run --rm i3-build bash -c '\
        if [ ! -f {{i3_build}}/build.ninja ]; then \
            meson setup {{i3_build}} {{i3_src}} \
                --buildtype=release -Ddocs=false -Dmans=false; \
        fi; \
        ninja -C {{i3_build}}'

# --- stage an install tree without touching the host system -----------------

# meson install --destdir into vendor/i3/build/install-root/usr/local/...
i3-stage: i3-build
    docker compose run --rm i3-build bash -c '\
        rm -rf {{i3_staged}}; \
        meson install -C {{i3_build}} \
            --destdir=/src/{{i3_staged}}'
    @echo "Staged at {{i3_staged}}/usr/local/"
    @ls {{i3_staged}}/usr/local/bin

# --- install / uninstall on the host ----------------------------------------

# sudo-copy the staged tree to /usr/local — stock /usr/bin/i3 untouched
i3-install: i3-stage
    @echo "Installing patched i3 to /usr/local (stock /usr/bin/i3 untouched)"
    # --remove-destination unlinks each in-use file before copying so a
    # running /usr/local/bin/i3 doesn't trigger ETXTBSY ("Text file busy").
    # The kernel keeps the running process attached to the old inode while
    # cp lands the new binary alongside it.
    sudo cp -a --remove-destination {{i3_staged}}/usr/local/. /usr/local/
    @echo "---"
    @which i3
    @i3 --version

# Remove patched files from /usr/local — fall back to stock /usr/bin/i3
i3-uninstall:
    @echo "Removing patched i3 from /usr/local (stock /usr/bin/i3 stays)"
    sudo rm -f \
        /usr/local/bin/i3 \
        /usr/local/bin/i3bar \
        /usr/local/bin/i3-config-wizard \
        /usr/local/bin/i3-dmenu-desktop \
        /usr/local/bin/i3-dump-log \
        /usr/local/bin/i3-input \
        /usr/local/bin/i3-migrate-config-to-v4 \
        /usr/local/bin/i3-msg \
        /usr/local/bin/i3-nagbar \
        /usr/local/bin/i3-save-tree \
        /usr/local/bin/i3-sensible-editor \
        /usr/local/bin/i3-sensible-pager \
        /usr/local/bin/i3-sensible-terminal \
        /usr/local/bin/i3-with-shmlog
    sudo rm -rf \
        /usr/local/etc/i3 \
        /usr/local/share/doc/i3 \
        /usr/local/share/xsessions/i3.desktop \
        /usr/local/share/xsessions/i3-with-shmlog.desktop \
        /usr/local/share/applications/i3.desktop \
        /usr/local/include/i3
    @echo "---"
    @which i3
    @i3 --version

# --- runtime ----------------------------------------------------------------

# i3-msg restart — swap running session to whichever i3 is first on PATH
i3-restart:
    i3-msg restart

# Show which i3 PATH resolves to and what version the running WM reports
i3-status:
    @printf 'On PATH : %s\n' "$(which i3)"
    @printf 'Binary  : %s\n' "$(i3 --version)"
    @printf 'Running : %s\n' "$(i3-msg -t get_version 2>/dev/null \
        | python3 -c 'import sys,json; v=json.load(sys.stdin); print(v["human_readable"])' \
        2>/dev/null || echo '(no running i3 IPC)')"

# Full pipeline: build → stage → install → restart in one step
i3-deploy: i3-install i3-restart

# --- cleanup ----------------------------------------------------------------

# Wipe vendor/i3/build entirely (inside container — build tree is root-owned)
i3-clean:
    docker compose run --rm i3-build rm -rf {{i3_build}}
