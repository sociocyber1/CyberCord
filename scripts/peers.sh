#!/usr/bin/env bash
# Start N terminal (headless) CyberCord nodes on this machine so you can test
# alone. Each node has its own data folder; nodes 2..N join through node 1.
#   scripts/peers.sh [N]    (default 3)    — Ctrl-C stops them all.
# Then open the window app too: it finds them through mDNS on the same machine.
set -euo pipefail
cd "$(dirname "$0")/.."
N="${1:-3}"
BIN="target/release/cybercord"; [ -x "$BIN" ] || BIN="target/debug/cybercord"
[ -x "$BIN" ] || { echo "build first: cargo build --release"; exit 1; }
mkdir -p run
pids=()
cleanup() { echo; echo "stopping…"; kill "${pids[@]}" 2>/dev/null || true; }
trap cleanup EXIT INT TERM
export CYBERCORD_ALLOW_LOOPBACK=1
( sleep 1000000 ) | CYBERCORD_DATA_DIR="run/peer1" "$BIN" --headless --name peer1 --tcp-port 47401 --quic-port 47401 > run/peer1.log 2>&1 &
pids+=("$!"); sleep 3
ID=$(grep -o "you are [A-Za-z0-9]*" run/peer1.log | awk '{print $3}')
BOOT="/ip4/127.0.0.1/udp/47401/quic-v1/p2p/$ID"
for i in $(seq 2 "$N"); do
  ( sleep 1000000 ) | CYBERCORD_DATA_DIR="run/peer$i" "$BIN" --headless --name "peer$i" --bootstrap "$BOOT" > "run/peer$i.log" 2>&1 &
  pids+=("$!")
done
echo "$N nodes running (logs in run/). Node 1: $BOOT"
tail -n +1 -f run/peer*.log
