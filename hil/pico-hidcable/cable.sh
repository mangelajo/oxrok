#!/bin/sh
# Sends commands to the Pico HID cable emulator and prints its answers.
# Usage: cable.sh "STATUS" ["PACKET 00..." ...]
PORT=${CABLE_PORT:-$(ls /dev/cu.usbmodemHIDCABLE* /dev/cu.usbmodemDS1EB* 2>/dev/null | head -1)}
[ -n "$PORT" ] || { echo "no emulator port"; exit 1; }
stty -f "$PORT" 115200 raw -echo
exec 3<>"$PORT"
for cmd in "$@"; do
  printf '%s\n' "$cmd" >&3
  sleep 0.3
done
timeout 1 cat <&3
exec 3>&-
