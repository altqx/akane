#!/usr/bin/env bash

set -Eeuo pipefail

need_cmd pm2

echo "Restarting PM2 app 'akane'…"
if pm2 describe "akane" >/dev/null 2>&1; then
  pm2 delete "akane" || true
fi

pm2 start ./akane --name "akane"
pm2 save || true

echo "Deployment complete. PM2 status for 'akane':"
pm2 status "akane" || true
