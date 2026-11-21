#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
bindir="$HOME/.local/bin"; appdir="$HOME/.local/share/applications"
icondir="$HOME/.local/share/icons/hicolor/256x256/apps"
mkdir -p "$bindir" "$appdir" "$icondir" "$HOME/.local/share/cybercord"
# The main build plays sound through ALSA (works with PipeWire/PulseAudio too).
if ldconfig -p 2>/dev/null | grep -q 'libasound\.so\.2' || [ -e /usr/lib/libasound.so.2 ] || [ -e /usr/lib64/libasound.so.2 ] || [ -e /usr/lib/x86_64-linux-gnu/libasound.so.2 ]; then
  install -m755 "$here/cybercord" "$bindir/cybercord"; sound="со звуком"
else
  install -m755 "$here/cybercord-nosound" "$bindir/cybercord"; sound="без звука (нет alsa-lib: sudo pacman -S alsa-lib, затем ./install.sh ещё раз)"
fi
install -m644 "$here/cybercord.png" "$icondir/cybercord.png"
sed "s|^Exec=cybercord$|Exec=$bindir/cybercord --name ${USER:-me}|" "$here/cybercord.desktop" > "$appdir/cybercord.desktop"
update-desktop-database "$appdir" 2>/dev/null || true
echo "Установлено ($sound). Запускайте «CyberCord» из меню приложений (имя: ${USER:-me}; поменять можно в $appdir/cybercord.desktop)."
if ! id -nG | grep -qw video; then
  echo "Для видео-кружков добавьте себя в группу video:  sudo usermod -aG video \$USER  (и перезайдите)."
fi
