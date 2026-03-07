#!/bin/sh
set -eu

: "${CODEXMANAGER_SERVICE_ADDR:=127.0.0.1:48760}"
: "${CODEXMANAGER_WEB_ADDR:=0.0.0.0:48761}"
: "${CODEXMANAGER_DB_DRIVER:=sqlite}"
: "${CODEXMANAGER_DATABASE_URL:=}"
: "${CODEXMANAGER_DB_PATH:=/tmp/codexmanager.db}"
: "${CODEXMANAGER_WEB_NO_OPEN:=1}"
: "${CODEXMANAGER_WEB_NO_SPAWN_SERVICE:=1}"

export CODEXMANAGER_SERVICE_ADDR
export CODEXMANAGER_WEB_ADDR
export CODEXMANAGER_DB_DRIVER
export CODEXMANAGER_DATABASE_URL
export CODEXMANAGER_DB_PATH
export CODEXMANAGER_WEB_NO_OPEN
export CODEXMANAGER_WEB_NO_SPAWN_SERVICE

if [ "$CODEXMANAGER_DB_DRIVER" = "sqlite" ]; then
  mkdir -p "$(dirname "$CODEXMANAGER_DB_PATH")"
fi

service_pid=""
web_pid=""
shutdown_in_progress="0"

cleanup() {
  if [ "$shutdown_in_progress" = "1" ]; then
    return
  fi
  shutdown_in_progress="1"
  if [ -n "$web_pid" ]; then
    kill "$web_pid" 2>/dev/null || true
  fi
  if [ -n "$service_pid" ]; then
    kill "$service_pid" 2>/dev/null || true
  fi
}

trap cleanup EXIT INT TERM

/usr/local/bin/codexmanager-service &
service_pid=$!
/usr/local/bin/codexmanager-web &
web_pid=$!

while :; do
  if ! kill -0 "$service_pid" 2>/dev/null; then
    wait "$service_pid" || true
    echo "codexmanager-service exited; stopping container" >&2
    cleanup
    wait "$web_pid" || true
    exit 1
  fi
  if ! kill -0 "$web_pid" 2>/dev/null; then
    wait "$web_pid" || true
    echo "codexmanager-web exited; stopping container" >&2
    cleanup
    wait "$service_pid" || true
    exit 1
  fi
  sleep 1
done
