#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_DIR="$(mktemp -d "${TMPDIR:-/tmp}/bixel-paypal-config.XXXXXX")"
trap 'rm -rf "$TEST_DIR"' EXIT

DEVELOPMENT_ENV="$TEST_DIR/.env.dev"
DEPLOY_ENV="$TEST_DIR/.env.deploy"

printf '%s\n' 'PAYPAL_CLIENT_ID=test_client_id_1234' > "$DEVELOPMENT_ENV"
printf '%s\n' 'PAYPAL_CLIENT_SECRET=test_secret_5678' >> "$DEVELOPMENT_ENV"
printf '%s\n' 'PAYPAL_ENV=sandbox' >> "$DEVELOPMENT_ENV"
printf '%s\n' 'DEVELOPMENT_TEAM=ABCDE12345' > "$DEPLOY_ENV"

(
  unset PAYPAL_CLIENT_ID PAYPAL_CLIENT_SECRET PAYPAL_ENV
  # shellcheck disable=SC1091
  source "$ROOT/scripts/load-build-env.sh"
  load_build_env "$DEVELOPMENT_ENV"
  [[ "$PAYPAL_CLIENT_ID" == "test_client_id_1234" ]]
  [[ "$PAYPAL_CLIENT_SECRET" == "test_secret_5678" ]]
  [[ "$PAYPAL_ENV" == "sandbox" ]]
)

development_output="$(
  BIXEL_DEV_ENV="$DEVELOPMENT_ENV" \
  "$ROOT/scripts/package-dmg.sh" --dry-run
)"
[[ "$development_output" == *"paypal:      …1234 (sandbox)"* ]]

CONFIG_PLIST="$TEST_DIR/Info.plist"
printf '%s\n' \
  '<?xml version="1.0" encoding="UTF-8"?>' \
  '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">' \
  '<plist version="1.0"><dict><key>PayPalClientID</key><string>$(PAYPAL_CLIENT_ID)</string></dict></plist>' \
  > "$CONFIG_PLIST"

BIXEL_INFO_PLIST_PATH="$CONFIG_PLIST" \
  BIXEL_DEV_ENV="$DEVELOPMENT_ENV" \
  CONFIGURATION=Debug \
  "$ROOT/scripts/configure-paypal-info.sh"
[[ "$(plutil -extract PayPalClientID raw -o - "$CONFIG_PLIST")" == "test_client_id_1234" ]]
[[ "$(plutil -extract PayPalClientSecret raw -o - "$CONFIG_PLIST")" == "test_secret_5678" ]]
[[ "$(plutil -extract PayPalEnv raw -o - "$CONFIG_PLIST")" == "sandbox" ]]

if ! rg -q 'static let lifetimeProductID = "bixel_ad_free"' "$ROOT/app/Bixel/Models/SubscriptionManager.swift"; then
  echo "SubscriptionManager must use product ID bixel_ad_free" >&2
  exit 1
fi

echo "PayPal configuration and build paths are valid"
