#!/usr/bin/env bash
# Cloud status skill entry point

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Pass all arguments to the Python script
python3 "$SCRIPT_DIR/check_status.py" "$@"
