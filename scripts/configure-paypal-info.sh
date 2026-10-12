#!/usr/bin/env bash
# Resolve PayPal configuration (Client ID, Secret, Environment) before Xcode processes Info.plist.
# Debug builds may use .env.dev for Xcode GUI builds; release builds/DMG packaging receive keys through environment.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INFO_PLIST_PATH="${BIXEL_INFO_PLIST_PATH:-$ROOT/app/Bixel/Info.plist}"
DEV_ENV_FILE="${BIXEL_DEV_ENV:-$ROOT/.env.dev}"

configured_client_id="${PAYPAL_CLIENT_ID:-}"
configured_client_secret="${PAYPAL_CLIENT_SECRET:-}"
configured_env="${PAYPAL_ENV:-sandbox}"

if [[ -z "${configured_client_id//[[:space:]]/}" && "${CONFIGURATION:-Debug}" == "Debug" && -f "$DEV_ENV_FILE" ]]; then
  # shellcheck disable=SC1091
  source "$ROOT/scripts/load-build-env.sh"
  load_build_env "$DEV_ENV_FILE"
  configured_client_id="${PAYPAL_CLIENT_ID:-}"
  configured_client_secret="${PAYPAL_CLIENT_SECRET:-}"
  configured_env="${PAYPAL_ENV:-sandbox}"
fi

if [[ ! -f "$INFO_PLIST_PATH" ]]; then
  printf 'error: Info.plist not found at %s\n' "$INFO_PLIST_PATH" >&2
  exit 1
fi

# Ensure PayPal keys exist in plist
set_or_add() {
  local key="$1"
  local val="$2"
  /usr/libexec/PlistBuddy -c "Set :$key $val" "$INFO_PLIST_PATH" 2>/dev/null \
    || /usr/libexec/PlistBuddy -c "Add :$key string $val" "$INFO_PLIST_PATH" 2>/dev/null || true
}

if [[ -n "${configured_client_id//[[:space:]]/}" ]]; then
  set_or_add "PayPalClientID" "$configured_client_id"
else
  set_or_add "PayPalClientID" '$(PAYPAL_CLIENT_ID)'
fi

if [[ -n "${configured_client_secret//[[:space:]]/}" ]]; then
  set_or_add "PayPalClientSecret" "$configured_client_secret"
else
  set_or_add "PayPalClientSecret" '$(PAYPAL_CLIENT_SECRET)'
fi

set_or_add "PayPalEnv" "$configured_env"
