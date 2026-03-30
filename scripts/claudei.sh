#!/usr/bin/env bash
set -euo pipefail

# claudei — launch Claude CLI in a sandboxed container with any project mounted
#
# Usage:
#   claudei [path]     Mount path (default: $PWD) as /workspace
#   claudei --help     Show this help

IMAGE="workspace-claude-cli:latest"

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    echo "Usage: claudei [project-path]"
    echo ""
    echo "Launch Claude CLI container with project-path mounted at /workspace."
    echo "Defaults to current directory if no path given."
    echo ""
    echo "Image: $IMAGE"
    exit 0
fi

# Resolve workspace to absolute path
WORKSPACE="${1:-$PWD}"
WORKSPACE="$(cd "$WORKSPACE" && pwd)" || { echo "Error: '$1' is not a valid directory" >&2; exit 1; }

# Verify image exists
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    echo "Error: Docker image '$IMAGE' not found." >&2
    echo "Build it first from reference/docker-workspace/" >&2
    exit 1
fi

# Pass through API key if set on host
API_KEY_ARGS=()
if [[ -n "${ANTHROPIC_API_KEY:-}" ]]; then
    API_KEY_ARGS=(-e "ANTHROPIC_API_KEY")
fi

echo "[claudei] Mounting: $WORKSPACE -> /workspace"
echo "[claudei] Image:    $IMAGE"

# Launch container — exec replaces this shell so signals go directly to container
exec docker run \
    --rm \
    -it \
    --read-only \
    --tmpfs /tmp:size=512M \
    --tmpfs /home/claude-user/.npm:size=256M \
    --tmpfs /run:size=64M \
    --security-opt=no-new-privileges:true \
    --memory=8g \
    --cpus=6.0 \
    -e TMPDIR=/tmp \
    -v claude-home:/home/claude-user \
    -v "${WORKSPACE}:/workspace" \
    "$IMAGE"
