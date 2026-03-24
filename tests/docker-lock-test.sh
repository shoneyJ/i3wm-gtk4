#!/usr/bin/env bash
# Integration test for i3more-lock running under Xvfb with mock PAM.
# Intended to run inside Docker via: docker compose run --rm test-lock
set -euo pipefail

DISPLAY=:99
export DISPLAY

PASS=0
FAIL=0

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1"; FAIL=$((FAIL + 1)); }

cleanup() {
    # Kill any lingering i3more-lock or Xvfb
    kill "$LOCK_PID" 2>/dev/null || true
    kill "$XVFB_PID" 2>/dev/null || true
    wait "$LOCK_PID" 2>/dev/null || true
    wait "$XVFB_PID" 2>/dev/null || true
}
trap cleanup EXIT

# ---------- Build ----------
echo "==> Building i3more-lock..."
cargo build --bin i3more-lock 2>&1
LOCK_BIN=./target/debug/i3more-lock

if [ ! -x "$LOCK_BIN" ]; then
    echo "ERROR: $LOCK_BIN not found or not executable"
    exit 1
fi

# ---------- Start Xvfb ----------
echo "==> Starting Xvfb on $DISPLAY..."
Xvfb "$DISPLAY" -screen 0 1920x1080x24 -ac &
XVFB_PID=$!
sleep 1

if ! kill -0 "$XVFB_PID" 2>/dev/null; then
    echo "ERROR: Xvfb failed to start"
    exit 1
fi

# ---------- Test 1: Successful unlock via pam_permit ----------
echo "==> Test 1: Unlock with pam_permit.so (any password should succeed)"

I3MORE_LOCK_PAM_SERVICE=i3more-lock-test "$LOCK_BIN" &
LOCK_PID=$!
sleep 2

if ! kill -0 "$LOCK_PID" 2>/dev/null; then
    fail "i3more-lock exited prematurely"
else
    pass "i3more-lock is running"

    # Type a password and press Enter
    xdotool key --delay 50 t e s t p a s s Return
    sleep 2

    if kill -0 "$LOCK_PID" 2>/dev/null; then
        fail "i3more-lock did not exit after correct password (pam_permit)"
        kill "$LOCK_PID" 2>/dev/null || true
        wait "$LOCK_PID" 2>/dev/null || true
    else
        wait "$LOCK_PID" 2>/dev/null
        EXIT_CODE=$?
        if [ "$EXIT_CODE" -eq 0 ]; then
            pass "i3more-lock exited with code 0 (unlock successful)"
        else
            fail "i3more-lock exited with code $EXIT_CODE (expected 0)"
        fi
    fi
fi

# ---------- Test 2: Escape clears buffer, lock persists ----------
echo "==> Test 2: Escape key clears buffer, lock screen stays active"

I3MORE_LOCK_PAM_SERVICE=i3more-lock-test "$LOCK_BIN" &
LOCK_PID=$!
sleep 2

if ! kill -0 "$LOCK_PID" 2>/dev/null; then
    fail "i3more-lock exited prematurely (test 2)"
else
    pass "i3more-lock is running (test 2)"

    # Type some characters then press Escape (should clear, not submit)
    xdotool key --delay 50 a b c Escape
    sleep 1

    if kill -0 "$LOCK_PID" 2>/dev/null; then
        pass "i3more-lock still running after Escape (buffer cleared)"
    else
        fail "i3more-lock exited unexpectedly after Escape"
    fi

    # Now submit with Enter to unlock
    xdotool key --delay 50 u n l o c k Return
    sleep 2

    if kill -0 "$LOCK_PID" 2>/dev/null; then
        fail "i3more-lock did not exit after password (test 2)"
        kill "$LOCK_PID" 2>/dev/null || true
        wait "$LOCK_PID" 2>/dev/null || true
    else
        pass "i3more-lock exited after password (test 2)"
    fi
fi

# ---------- Test 3: Empty Enter does not unlock ----------
echo "==> Test 3: Empty password (just Enter) does not unlock"

I3MORE_LOCK_PAM_SERVICE=i3more-lock-test "$LOCK_BIN" &
LOCK_PID=$!
sleep 2

if ! kill -0 "$LOCK_PID" 2>/dev/null; then
    fail "i3more-lock exited prematurely (test 3)"
else
    pass "i3more-lock is running (test 3)"

    # Press Enter with empty password
    xdotool key Return
    sleep 1

    if kill -0 "$LOCK_PID" 2>/dev/null; then
        pass "i3more-lock stays locked on empty Enter"
    else
        fail "i3more-lock exited on empty Enter (should not unlock)"
    fi

    # Clean up - unlock for real
    xdotool key --delay 50 x Return
    sleep 2
    kill "$LOCK_PID" 2>/dev/null || true
    wait "$LOCK_PID" 2>/dev/null || true
fi

# ---------- Summary ----------
echo ""
echo "==> Results: $PASS passed, $FAIL failed"

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
