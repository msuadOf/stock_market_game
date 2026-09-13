#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
required_node="$(<"$repo_root/.nvmrc")"
node_bin="${NODE_BIN:-}"

if [[ -d "$node_bin" ]]; then
    node_command="$node_bin/node"
    corepack_command="$node_bin/corepack"
elif [[ -n "$node_bin" ]]; then
    node_command="$node_bin"
    corepack_command="$(dirname "$node_bin")/corepack"
else
    node_command="$(command -v node 2>/dev/null || true)"
    corepack_command="$(command -v corepack 2>/dev/null || true)"
fi

current_node="${node_command:+$($node_command --version 2>/dev/null || true)}"

if [[ "$current_node" != "v$required_node" ]]; then
    printf 'Node.js %s is required for Corepack pnpm; current version is %s. Activate the version in .nvmrc, then rerun this command.\n' \
        "$required_node" "${current_node:-unavailable}" >&2
    exit 1
fi

if [[ -z "$corepack_command" || ! -x "$corepack_command" ]]; then
    printf 'Corepack is required to run the repository-pinned pnpm. Enable or install Corepack for Node.js %s, then rerun this command.\n' \
        "$required_node" >&2
    exit 1
fi

cd "$repo_root"
exec "$corepack_command" pnpm "$@"
