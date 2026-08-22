#!/usr/bin/env bash
# Spike A host runner. Started via `nix run .#spike-a`.
# Subcommands: prove | shell | stop | destroy
set -euo pipefail

STATE="${SNOWBOX_SPIKE_DIR:-$PWD/.snowbox-spike}"
mkdir -p "$STATE"
DISK_ABS="$STATE/workspace.img"
SOCK_ABS="$STATE/vfkit.sock"
LOG_ABS="$STATE/serial.log"
PIDFILE="$STATE/vfkit.pid"

# vfkit wants absolute paths.
DISK_ABS="$(cd "$(dirname "$DISK_ABS")" && pwd)/$(basename "$DISK_ABS")"
SOCK_ABS="$(cd "$(dirname "$SOCK_ABS")" && pwd)/$(basename "$SOCK_ABS")"
LOG_ABS="$(cd "$(dirname "$LOG_ABS")" && pwd)/$(basename "$LOG_ABS")"

ensure_disk() {
  if [ ! -e "$DISK_ABS" ]; then
    # Sparse 512MiB raw image. Guest autoFormats ext4 on first boot.
    truncate -s 512M "$DISK_ABS"
  fi
}

run_vfkit() {
  extra_cmdline="${1:-}"
  serial_device="${2:-virtio-serial,stdio}"
  cmdline="init=${TOPLEVEL}/init console=hvc0 reboot=t panic=-1 ${extra_cmdline}"
  exec vfkit \
    --cpus 2 \
    --memory 2048 \
    --bootloader "linux,kernel=${KERNEL},initrd=${INITRD},cmdline=\"${cmdline}\"" \
    --device virtio-rng \
    --device "virtio-blk,path=${DISK_ABS}" \
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
  rmdir "$STATE" 2>/dev/null || true
}

cmd_shell() {
  if is_running; then
    echo "already running (pid $(cat "$PIDFILE")); stop first" >&2
    exit 1
  fi
  ensure_disk
  rm -f "$SOCK_ABS"
  echo "serial console on this terminal. Host \$HOME is not in the guest." >&2
  echo "workspace is /workspace on the virtio-blk disk." >&2
  run_vfkit "" "virtio-serial,stdio"
}

cmd_prove() {
  if is_running; then
    echo "already running; stop first" >&2
    exit 1
  fi
  ensure_disk
  rm -f "$SOCK_ABS" "$LOG_ABS"
  : >"$LOG_ABS"

  run_vfkit "spike.prove=1" "virtio-serial,logFilePath=${LOG_ABS}" &
  echo $! >"$PIDFILE"
  pid=$(cat "$PIDFILE")

  echo "vfkit pid $pid; waiting for SPIKE_A_PASS (180s)" >&2
  deadline=$((SECONDS + 180))
  while ((SECONDS < deadline)); do
    if grep -q 'SPIKE_A_PASS' "$LOG_ABS" 2>/dev/null; then
      echo "guest reported SPIKE_A_PASS" >&2
      # wait for poweroff
      for _ in $(seq 1 40); do
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.5
      done
      cmd_stop
      if command -v debugfs >/dev/null 2>&1; then
        if debugfs -R 'stat /marker' "$DISK_ABS" 2>/dev/null | grep -q marker; then
          echo "debugfs: /workspace/marker present on guest disk" >&2
        else
          echo "debugfs: could not stat /marker (may still be unmounted); serial pass stands" >&2
        fi
      fi
      echo "SPIKE_A_HOST_PASS"
      return 0
    fi
    if grep -q 'SPIKE_A_FAIL' "$LOG_ABS" 2>/dev/null; then
      echo "guest failed:" >&2
      grep 'SPIKE_A_FAIL' "$LOG_ABS" >&2 || true
      cmd_stop
      exit 1
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "vfkit exited before PASS. serial log:" >&2
      cat "$LOG_ABS" >&2 || true
      rm -f "$PIDFILE"
      exit 1
    fi
    sleep 1
  done
  echo "timeout. serial log:" >&2
  cat "$LOG_ABS" >&2 || true
  cmd_stop
  exit 1
}

usage() {
  echo "usage: spike-a prove|shell|stop|destroy" >&2
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
