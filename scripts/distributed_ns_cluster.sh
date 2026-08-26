#!/usr/bin/env bash
# Distributed TCP training across REAL Linux network namespaces.
#
# Topology:
#
#   root netns                          worker netns (per i in 0..1)
#   ───────────────────────────         ─────────────────────────────
#   coordinator role of the e2e  ◀────  worker role of the same test
#   test binary, bound to               binary, dialing SPECTRA_DIST_COORDINATOR_ADDR
#   SPECTRA_DIST_BIND=0.0.0.0:PORT      (= this side of its veth) — loopback in the
#   on ALL interfaces incl. veth        worker ns is isolated, so traffic provably
#                                        crosses a real routed link, not 127.0.0.1.
#
#   root side vr<i>: 10.77.<20+i>.1/30  ◀──veth──▶  worker side vw<i>: 10.77.<20+i>.2/30
#
# Requires: root (or sudo), iproute2 (`ip netns`), jq + cargo for building.
# Everything is torn down by an EXIT trap; the run itself is time-bounded.
set -euo pipefail

readonly TEST_NAME="stdlib::ml_distributed_tcp::dist_tcp_fault_tests::dist_tcp_e2e_two_os_processes_over_real_tcp"
readonly PORT="${SPECTRA_DIST_PORT:-47232}"
readonly WORKERS=(worker0 worker1)
readonly LINK_UP_TIMEOUT_SECS=10
readonly COORD_TIMEOUT_SECS="${SPECTRA_NS_COORD_TIMEOUT:-300}"

LOG_DIR="${SPECTRA_NS_LOG_DIR:-$(mktemp -d /tmp/spectra-dist-ns.XXXXXX)}"
mkdir -p "$LOG_DIR"

COORD_PID=""
cleanup() {
    local rc=$?
    trap - EXIT
    if [ -n "$COORD_PID" ] && kill -0 "$COORD_PID" 2>/dev/null; then
        kill "$COORD_PID" 2>/dev/null || true
        wait "$COORD_PID" 2>/dev/null || true
    fi
    for ns in "${WORKERS[@]}"; do
        # Deleting the netns also destroys its veth peer.
        ip netns del "$ns" >/dev/null 2>&1 || true
    done
    echo "── cluster logs (${LOG_DIR}) ──"
    for log in "$LOG_DIR"/*.log; do
        [ -e "$log" ] || continue
        echo "──────── $(basename "$log") ────────"
        cat "$log"
    done
    exit "$rc"
}
trap cleanup EXIT

if [ "$(id -u)" -ne 0 ]; then
    echo "error: must run as root (try: sudo -E bash $0)" >&2
    exit 1
fi
command -v ip >/dev/null || { echo "error: iproute2 ('ip') required" >&2; exit 1; }

if [ -z "${DIST_TEST_BIN:-}" ]; then
    echo "── building lib test binary (cargo test -p spectra-runtime --lib --no-run) ──"
    cargo test -p spectra-runtime --lib --no-run
    DIST_TEST_BIN="$(cargo test -p spectra-runtime --lib --no-run --message-format=json \
        | jq -r 'select(.executable != null) | .executable' | head -n1)"
fi
[ -x "$DIST_TEST_BIN" ] || { echo "error: test binary '$DIST_TEST_BIN' not executable" >&2; exit 1; }
echo "test binary: $DIST_TEST_BIN"

# Build the namespace/link topology first so links are up before anything dials.
declare -A GW=()
for i in "${!WORKERS[@]}"; do
    ns="${WORKERS[$i]}"
    subnet="10.77.$((20 + i))"
    gw_ip="${subnet}.1"
    ns_ip="${subnet}.2"
    root_veth="vr$i"
    ns_veth="vw$i"
    GW[$ns]="$gw_ip"

    ip netns add "$ns"
    ip link add "$root_veth" type veth peer name "$ns_veth"
    ip link set "$ns_veth" netns "$ns"
    ip addr add "${gw_ip}/30" dev "$root_veth"
    ip link set "$root_veth" up
    ip netns exec "$ns" ip addr add "${ns_ip}/30" dev "$ns_veth"
    ip netns exec "$ns" ip link set "$ns_veth" up
    ip netns exec "$ns" ip link set lo up
    ip netns exec "$ns" ip route replace default via "$gw_ip"
    echo "netns $ns: ${ns_ip}/30 via $gw_ip (coordinator address from this ns)"
done

# Coordinator in the ROOT namespace, reachable from every worker netns.
echo "── starting coordinator on 0.0.0.0:${PORT} (root ns) ──"
timeout "$COORD_TIMEOUT_SECS" env \
    SPECTRA_DIST_ROLE=coordinator \
    SPECTRA_DIST_BIND=0.0.0.0 \
    SPECTRA_DIST_PORT="$PORT" \
    "$DIST_TEST_BIN" -- --exact "$TEST_NAME" --nocapture \
    >"$LOG_DIR/coordinator.log" 2>&1 &
COORD_PID=$!

for _ in $(seq 1 50); do
    ss -ltn 2>/dev/null | grep -q ":${PORT} " && break
    if ! kill -0 "$COORD_PID" 2>/dev/null; then
        echo "error: coordinator exited before listening on :${PORT}" >&2
        tail -50 "$LOG_DIR/coordinator.log" >&2 || true
        exit 1
    fi
    sleep 0.2
done
ss -ltn 2>/dev/null | grep -q ":${PORT} " \
    || { echo "error: coordinator never listened on :${PORT}" >&2; exit 1; }
echo "coordinator is listening on 0.0.0.0:${PORT}"

# Pre-flight: prove each namespace can actually reach the coordinator over its
# routed veth BEFORE blaming the training protocol for connectivity failures.
deadline=$((SECONDS + LINK_UP_TIMEOUT_SECS))
for ns in "${WORKERS[@]}"; do
    until ip netns exec "$ns" bash -c "exec 3<>/dev/tcp/${GW[$ns]}/${PORT}" 2>/dev/null; do
        if [ "$SECONDS" -ge "$deadline" ]; then
            echo "error: netns $ns cannot reach ${GW[$ns]}:${PORT}" >&2
            exit 1
        fi
        sleep 0.2
    done
    echo "netns $ns → ${GW[$ns]}:${PORT} connectivity OK"
done

# Workers inside their namespaces. Loopback inside each ns is isolated, so the
# SPECTRA_DIST_COORDINATOR_ADDR override is what makes attach possible at all.
worker_pids=()
for i in "${!WORKERS[@]}"; do
    ns="${WORKERS[$i]}"
    ip netns exec "$ns" env \
        SPECTRA_DIST_ROLE=worker \
        SPECTRA_DIST_WORKER_ID="$i" \
        SPECTRA_DIST_PORT="$PORT" \
        SPECTRA_DIST_COORDINATOR_ADDR="${GW[$ns]}" \
        timeout "$COORD_TIMEOUT_SECS" "$DIST_TEST_BIN" -- --exact "$TEST_NAME" --nocapture \
        >"$LOG_DIR/${ns}.log" 2>&1 &
    worker_pids+=($!)
    echo "launched $ns (pid ${worker_pids[-1]}) → coordinator ${GW[$ns]}:$PORT"
done

echo "── waiting for coordinator (training runs to completion in the root ns) ──"
wait "$COORD_PID"
echo "coordinator finished OK"

rc=0
for i in "${!WORKERS[@]}"; do
    if wait "${worker_pids[$i]}"; then
        echo "${WORKERS[$i]} finished OK"
    else
        echo "${WORKERS[$i]} FAILED" >&2
        rc=1
    fi
done
exit "$rc"
