#!/usr/bin/env bash
set -euo pipefail
rm -f "$HOME/.local/bin/cybercord" "$HOME/.local/share/applications/cybercord.desktop" "$HOME/.local/share/icons/hicolor/256x256/apps/cybercord.png" "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/cybercord.desktop"
echo "Удалено. Ваш ID, друзья и чаты в ~/.local/share/cybercord сохранены."
