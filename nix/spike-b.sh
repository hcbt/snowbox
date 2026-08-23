#!/usr/bin/env bash
# Spike B: Host Environment + Cache + copy into the guest.
# Subcommands: prove | shell | stop | destroy
set -euo pipefail

STATE="${SNOWBOX_SPIKE_DIR:-$PWD/.snowbox-spike-b}"
mkdir -p "$STATE"
DISK_ABS="$(cd "$STATE" && pwd)/workspace.img"
SOCK_ABS="$(cd "$STATE" && pwd)/vfkit.sock"
LOG_ABS="$(cd "$STATE" && pwd)/serial.log"
PIDFILE="$(cd "$STATE" && pwd)/vfkit.pid"
CACHE="$(cd "$STATE" && pwd)/cache"
HELLO="${HELLO}"
MAC="52:54:00:53:4e:42"
KEY="$STATE/id_ed25519"
if [ ! -f "$KEY" ]; then
  cp "$SPIKE_B_KEY_SRC" "$KEY"
  chmod 600 "$KEY"
fi

mkdir -p "$CACHE"

ensure_disk() {
  if [ ! -e "$DISK_ABS" ]; then
    truncate -s 512M "$DISK_ABS"
  fi
}

run_vfkit() {
  serial_device="${1:-virtio-serial,stdio}"
  cmdline="init=${TOPLEVEL}/init console=hvc0 reboot=t panic=-1"
  exec vfkit \
    --cpus 2 \
    --memory 2048 \
    --bootloader "linux,kernel=${KERNEL},initrd=${INITRD},cmdline=\"${cmdline}\"" \
    --device virtio-rng \
    --device "virtio-blk,path=${DISK_ABS}" \
    --device "virtio-net,nat,mac=${MAC}" \
    --device "${serial_device}" \
    --restful-uri "unix://${SOCK_ABS}"
}

is_running() {
  if [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
    return 0
  fi
  return 1
}

cmd_stop() {
  if [ -S "$SOCK_ABS" ]; then
    curl -sS --unix-socket "$SOCK_ABS" \
      -X POST \
      -H 'Content-Type: application/json' \
      -d '{"state":"Stop"}' \
      http://localhost/vm/state >/dev/null 2>&1 || true
  fi
  if [ -f "$PIDFILE" ]; then
    pid=$(cat "$PIDFILE")
    kill "$pid" 2>/dev/null || true
    for _ in 1 2 3 4 5 6 7 8 9 10; do
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.3
    done
    kill -9 "$pid" 2>/dev/null || true
    rm -f "$PIDFILE"
  fi
  rm -f "$SOCK_ABS"
}

cmd_destroy() {
  cmd_stop
  rm -f "$DISK_ABS" "$LOG_ABS"
  rm -rf "$CACHE"
  rmdir "$STATE" 2>/dev/null || true
}

SSH_OPTS=(
  -i "$KEY"
  -o StrictHostKeyChecking=no
  -o UserKnownHostsFile=/dev/null
  -o IdentitiesOnly=yes
  -o BatchMode=yes
  -o ConnectTimeout=5
)

guest_ssh() {
  ip="$1"
  shift
  ssh "${SSH_OPTS[@]}" "root@${ip}" "$@"
}

wait_ready() {
  # Do not capture vfkit stdout via command substitution — it stays open
  # until the VM stops. Write the IP to a file instead.
  : >"$LOG_ABS"
  rm -f "$STATE/guest.ip"
  run_vfkit "virtio-serial,logFilePath=${LOG_ABS}" >/dev/null 2>&1 &
  echo $! >"$PIDFILE"
  pid=$(cat "$PIDFILE")
  echo "vfkit pid $pid; waiting for SPIKE_B_READY (180s)" >&2
  deadline=$((SECONDS + 180))
  while ((SECONDS < deadline)); do
    if grep -q 'SPIKE_B_FAIL' "$LOG_ABS" 2>/dev/null; then
      echo "guest failed:" >&2
      grep 'SPIKE_B_FAIL' "$LOG_ABS" >&2 || true
      cmd_stop
      exit 1
    fi
    if grep -q 'SPIKE_B_READY' "$LOG_ABS" 2>/dev/null; then
      ip=$(grep -o 'SPIKE_B_IP=[0-9.]*' "$LOG_ABS" | tail -1 | cut -d= -f2)
      if [ -z "$ip" ]; then
        echo "READY without IP" >&2
        cmd_stop
        exit 1
      fi
      echo "guest ready at $ip" >&2
      echo "$ip" >"$STATE/guest.ip"
      return 0
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "vfkit exited before READY. serial log:" >&2
      cat "$LOG_ABS" >&2 || true
      rm -f "$PIDFILE"
      exit 1
    fi
    sleep 1
  done
  echo "timeout waiting for READY. serial log:" >&2
  cat "$LOG_ABS" >&2 || true
  cmd_stop
  exit 1
}

realize_into_cache() {
  echo "realizing $HELLO into Cache $CACHE" >&2
  nix copy --to "file://${CACHE}" "$HELLO"
}

wait_ssh() {
  ip="$1"
  echo "waiting for ssh at $ip" >&2
  for _ in $(seq 1 40); do
    if ssh "${SSH_OPTS[@]}" "root@${ip}" true >/dev/null 2>&1; then
      echo "ssh ok" >&2
      return 0
    fi
    sleep 1
  done
  echo "ssh never came up at $ip" >&2
  ssh "${SSH_OPTS[@]}" -v "root@${ip}" true >&2 || true
  ping -c 2 "$ip" >&2 || true
  exit 1
}

copy_to_guest() {
  ip="$1"
  echo "copying $HELLO from Cache to guest $ip (no substituters)" >&2
  NIX_SSHOPTS="${SSH_OPTS[*]}"
  export NIX_SSHOPTS
  nix copy \
    --from "file://${CACHE}" \
    --to "ssh://root@${ip}" \
    --option substituters "file://${CACHE}" \
    --option extra-substituters "" \
    "$HELLO"
}

activate_hello() {
  ip="$1"
  guest_ssh "$ip" nix-env -i "$HELLO"
}

hello_works() {
  ip="$1"
  guest_ssh "$ip" /root/.nix-profile/bin/hello
}

cmd_shell() {
  if is_running; then
    echo "already running (pid $(cat "$PIDFILE")); stop first" >&2
    exit 1
  fi
  ensure_disk
  rm -f "$SOCK_ABS"
  echo "serial console. ssh key is nix/spike-b/id_ed25519" >&2
  run_vfkit "virtio-serial,stdio"
}

cmd_prove() {
  trap cmd_stop EXIT
  if is_running; then
    echo "already running; stop first" >&2
    exit 1
  fi
  ensure_disk
  rm -f "$SOCK_ABS" "$LOG_ABS"
  mkdir -p "$CACHE"

  realize_into_cache
  nar_after_realize=$(find "$CACHE" -name '*.narinfo' | wc -l | tr -d ' ')
  echo "cache narinfo count after realize: $nar_after_realize" >&2
  if [ "$nar_after_realize" -lt 1 ]; then
    echo "Cache empty after realize" >&2
    exit 1
  fi

  wait_ready
  ip=$(cat "$STATE/guest.ip")
  wait_ssh "$ip"

  # Live add: hello is not in the guest image; copy + nix-env on the running VM.
  copy_to_guest "$ip"
  activate_hello "$ip"
  out=$(hello_works "$ip")
  echo "live add: $out" >&2
  echo "$out" | grep -q 'Hello, world!' || {
    echo "hello did not run after live add" >&2
    cmd_stop
    exit 1
  }

  guest_ssh "$ip" 'echo keepme > /workspace/keepme'
  guest_ssh "$ip" 'printf "%s\n" "[user]" "	email = spike@snowbox" > /root/.gitconfig'
  guest_ssh "$ip" 'mkdir -p /root/.local/bin && echo nasty > /root/.local/bin/nasty && chmod +x /root/.local/bin/nasty'
  guest_ssh "$ip" test -x /root/.local/bin/nasty

  # Reset: Environment re-applied; Workspace + .gitconfig kept; ~/.local gone.
  guest_ssh "$ip" rm -rf /root/.local
  activate_hello "$ip"
  guest_ssh "$ip" test -f /workspace/keepme
  guest_ssh "$ip" grep -q spike@snowbox /root/.gitconfig
  if guest_ssh "$ip" test -e /root/.local/bin/nasty; then
    echo "nasty survived reset" >&2
    cmd_stop
    exit 1
  fi
  hello_works "$ip" >/dev/null
  echo "reset: workspace+gitconfig kept, nasty gone, hello still runs" >&2

  # Guest must not see the Host Cache path.
  if guest_ssh "$ip" test -e "$CACHE"; then
    echo "guest can see Host Cache path $CACHE" >&2
    cmd_stop
    exit 1
  fi
  echo "guest cannot see Host Cache path" >&2

  cmd_stop
  echo "sandbox 1 stopped; Cache kept" >&2

  # Second sandbox: copy from Cache only (substituters = Cache). Workspace
  # disk is reused (not the point); Cache narinfo count must not grow.
  wait_ready
  ip=$(cat "$STATE/guest.ip")
  wait_ssh "$ip"
  copy_to_guest "$ip"
  activate_hello "$ip"
  hello_works "$ip" >/dev/null
  nar_after_second=$(find "$CACHE" -name '*.narinfo' | wc -l | tr -d ' ')
  echo "cache narinfo count after second sandbox: $nar_after_second" >&2
  if [ "$nar_after_second" -ne "$nar_after_realize" ]; then
    echo "Cache grew on second sandbox (re-fetched?)" >&2
    cmd_stop
    exit 1
  fi
  echo "second sandbox copied from Cache; narinfo count unchanged" >&2

  cmd_destroy
  echo "SPIKE_B_HOST_PASS"
}

usage() {
  echo "usage: spike-b prove|shell|stop|destroy" >&2
  exit 2
}

cmd="${1:-}"
case "$cmd" in
  prove) cmd_prove ;;
  shell) cmd_shell ;;
  stop) cmd_stop ;;
  destroy) cmd_destroy ;;
  *) usage ;;
esac
