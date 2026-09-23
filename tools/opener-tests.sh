#!/usr/bin/env bash
# Runs the integration tests in tests/opener.rs against the OpENer sample adapter.
set -euo pipefail

cd "$(dirname "$0")/.."

usage() {
    cat <<'USAGE'
Usage: tools/opener-tests.sh [OPTIONS] [-- CARGO_TEST_ARGS...]

Starts the OpENer sample adapter in Docker (using tools/adapter.sh from an
OpENer checkout with the Docker setup), then runs the ignored integration
tests in tests/opener.rs against it.

Options:
  --no-start     use an adapter that is already running
  --down         stop the adapter afterward
  -h, --help     show this help

Environment:
  OPENER_DIR        OpENer checkout (default ../OpENer)
  EIP_TEST_ADAPTER  adapter address (default 10.44.18.3)
  EIP_TEST_EDS      sample EDS (default $OPENER_DIR/data/opener_sample_app.eds)

Arguments after -- go to cargo test, e.g. a test name filter:
  tools/opener-tests.sh -- config_data
USAGE
}

start=1
down=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        -h | --help) usage; exit 0 ;;
        --no-start) start=0 ;;
        --down) down=1 ;;
        --) shift; break ;;
        *) echo "unknown option: $1" >&2; usage >&2; exit 1 ;;
    esac
    shift
done

opener_dir="${OPENER_DIR:-../OpENer}"
export EIP_TEST_EDS="${EIP_TEST_EDS:-$opener_dir/data/opener_sample_app.eds}"

if [[ $start -eq 1 ]]; then
    if [[ ! -x "$opener_dir/tools/adapter.sh" ]]; then
        echo "no $opener_dir/tools/adapter.sh; set OPENER_DIR to an OpENer checkout" \
             "with the Docker setup (branch jgleaton/feature-docker)" >&2
        exit 1
    fi
    "$opener_dir/tools/adapter.sh" up
fi

status=0
cargo test --test opener -- --ignored "$@" || status=$?

if [[ $down -eq 1 ]]; then
    "$opener_dir/tools/adapter.sh" down
fi
exit $status
