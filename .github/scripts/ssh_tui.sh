#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# by dirstats contributors
#
# Runs the dirstats binary over SSH to this same machine, the way someone
# on a text console or a plain SSH session would, and checks it picks the
# right interface by itself (see crates/dirstats/src/session.rs):
#
#   - with a terminal: the terminal interface, and `q` quits it;
#   - without one: the summary, with a note saying why;
#   - Linux, DISPLAY pointing at no X server: the window fails to open and
#     the terminal interface takes over, rather than a crash.
#
# CI only (macOS and Linux): it starts its own sshd as root on port 2222
# and needs passwordless sudo. Usage: ssh_tui.sh [path/to/dirstats]
set -euo pipefail

bin=$(cd "$(dirname "${1:-target/debug/dirstats}")" && pwd)/$(basename "${1:-target/debug/dirstats}")
[ -x "$bin" ] || { echo "no dirstats binary at $bin" >&2; exit 1; }
work=$(mktemp -d)
port=2222

mkdir -p "$work/tree/sub"
echo hello >"$work/tree/sub/file.txt"
echo world >"$work/tree/top.txt"

ssh-keygen -q -t ed25519 -N '' -f "$work/host_key"
ssh-keygen -q -t ed25519 -N '' -f "$work/client_key"
cp "$work/client_key.pub" "$work/authorized_keys"
chmod 600 "$work/authorized_keys"
# PAM stays on so a runner account without a usable password can still log
# in with a key; StrictModes is off because the keys live under /tmp.
cat >"$work/sshd_config" <<EOF
Port $port
ListenAddress 127.0.0.1
HostKey $work/host_key
PidFile $work/sshd.pid
AuthorizedKeysFile $work/authorized_keys
AllowUsers $(id -un)
PubkeyAuthentication yes
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM yes
StrictModes no
X11Forwarding no
EOF

if [ "$(uname)" = Linux ]; then
    command -v sshd >/dev/null || [ -x /usr/sbin/sshd ] || sudo apt-get install -y -qq openssh-server >/dev/null
    sudo mkdir -p /run/sshd
fi
sshd=$(command -v sshd || echo /usr/sbin/sshd)
sudo "$sshd" -t -f "$work/sshd_config"
sudo "$sshd" -f "$work/sshd_config" -E "$work/sshd.log"
cleanup() {
    sudo kill "$(cat "$work/sshd.pid" 2>/dev/null)" 2>/dev/null || true
    rm -rf "$work"
}
trap cleanup EXIT

ssh_opts=(-i "$work/client_key" -p "$port" -o BatchMode=yes -o StrictHostKeyChecking=no
    -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ForwardX11=no)
host="$(id -un)@127.0.0.1"

# Wait for sshd to accept a login.
for _ in $(seq 20); do
    ssh "${ssh_opts[@]}" -T "$host" true 2>/dev/null && break
    sleep 0.5
done
ssh "${ssh_opts[@]}" -T "$host" true || { cat "$work/sshd.log" >&2; exit 1; }

# Runs `$1` remotely and writes everything it printed to $out. With a
# terminal (-tt), `q` is typed after a pause so the interface has drawn.
# Fails after 60 s, so a hung interface fails the job instead of stalling it.
out="$work/out"
remote() {
    local mode=$1 command=$2 status=0
    if [ "$mode" = tty ]; then
        { sleep 5; printf q; sleep 5; } | ssh "${ssh_opts[@]}" -tt "$host" "$command" >"$out" 2>&1 &
    else
        ssh "${ssh_opts[@]}" -T "$host" "$command" </dev/null >"$out" 2>&1 &
    fi
    local pid=$!
    ( sleep 60; kill "$pid" 2>/dev/null ) &
    local watchdog=$!
    wait "$pid" || status=$?
    kill "$watchdog" 2>/dev/null || true
    # Reap it quietly; bash otherwise reports the kill as "Terminated".
    wait "$watchdog" 2>/dev/null || true
    return "$status"
}

alt_screen=$'\e[?1049h'
failed=0
check() {
    local name=$1 ok=$2
    if [ "$ok" = yes ]; then
        echo "ok: $name"
    else
        echo "FAIL: $name; it printed:" >&2
        LC_ALL=C sed -e 's/\x1b/\\e/g' "$out" | head -c 2000 >&2
        echo >&2
        failed=1
    fi
}
has() { grep -qaF -- "$1" "$out"; }

# 1. A plain SSH session with a terminal: the terminal interface, no window.
ok=yes
remote tty "'$bin' '$work/tree'" || ok=no
has "$alt_screen" || ok=no
has "could not open a window" && ok=no
check "ssh with a terminal opens the terminal interface" "$ok"

# 2. No terminal at all: the summary, and a note saying why.
ok=yes
remote plain "'$bin' '$work/tree'" || ok=no
has "no graphical session or terminal" || ok=no
has "top.txt" || ok=no
has "$alt_screen" && ok=no
check "ssh without a terminal prints the summary" "$ok"

# 3. Linux with a DISPLAY that leads nowhere: the window fails, the
# terminal interface takes over.
if [ "$(uname)" = Linux ]; then
    ok=yes
    remote tty "DISPLAY=:99 '$bin' '$work/tree'" || ok=no
    has "could not open a window" || ok=no
    has "$alt_screen" || ok=no
    check "a dead DISPLAY falls back to the terminal interface" "$ok"
fi

exit "$failed"
