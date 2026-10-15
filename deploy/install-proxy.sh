#!/usr/bin/env bash
# Run from the unpacked project directory as root, after docker compose up.
set -euo pipefail
[ "$(id -u)" -eq 0 ] || { echo 'Run as root'; exit 1; }
command -v nginx >/dev/null || { echo 'Existing Nginx is required'; exit 1; }
nginx -t
container=$(docker compose -p waifu-sync ps -q sync)
[ -n "$container" ] || { echo 'Start the waifu-sync container first'; exit 1; }
[ "$(docker inspect --format '{{.State.Running}}' "$container")" = true ] || { echo 'The waifu-sync container is not running'; exit 1; }
curl --fail --silent http://127.0.0.1:8787/health
printf '\n'
site=/etc/nginx/sites-available/waifu-sync.conf
link=/etc/nginx/sites-enabled/waifu-sync.conf
[ -d /etc/nginx/sites-enabled ] || { echo 'Nonstandard Nginx layout. Install the provided config manually.'; exit 1; }
[ ! -e "$site" ] && [ ! -L "$site" ] && [ ! -e "$link" ] && [ ! -L "$link" ] || { echo 'Config already exists. Review it instead of overwriting.'; exit 1; }
current_config=$(nginx -T 2>/dev/null)
if grep -F 'waifudownload.hopto.org' <<< "$current_config" >/dev/null; then
  echo 'Domain already occurs in Nginx configuration. Review it before installing.'; exit 1
fi
install -m 644 deploy/waifudownload.nginx.conf "$site"
ln -s "$site" "$link"
if ! nginx -t; then
  rm -- "$link" "$site"
  echo 'New configuration removed. Existing Nginx has not been reloaded.'
  exit 1
fi
systemctl reload nginx
echo 'HTTP proxy is ready. Configure the HTTPS certificate before signing in.'
