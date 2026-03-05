#!/bin/sh
set -eu

: "${CODEXMANAGER_SERVICE_ADDR:=127.0.0.1:48760}"
: "${CODEXMANAGER_WEB_ADDR:=0.0.0.0:48761}"
: "${CODEXMANAGER_DB_PATH:=/tmp/codexmanager.db}"
: "${CODEXMANAGER_WEB_NO_OPEN:=1}"
: "${CODEXMANAGER_WEB_NO_SPAWN_SERVICE:=1}"

export CODEXMANAGER_SERVICE_ADDR
export CODEXMANAGER_WEB_ADDR
export CODEXMANAGER_DB_PATH
export CODEXMANAGER_WEB_NO_OPEN
export CODEXMANAGER_WEB_NO_SPAWN_SERVICE

mkdir -p "$(dirname "$CODEXMANAGER_DB_PATH")"

/usr/local/bin/codexmanager-service &
service_pid=$!

cleanup() {
  kill "$service_pid" 2>/dev/null || true
}

trap cleanup EXIT INT TERM

exec /usr/local/bin/codexmanager-web
