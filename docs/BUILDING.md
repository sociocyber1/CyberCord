# Сборка

Нужен свежий стабильный Rust (`rustup`; проверено на 1.97).

```bash
git clone https://github.com/sociocyber1/CyberCord
cd CyberCord
cargo build --release                           # чат, GIF, эмодзи, музыка (без звука)
cargo build --release --features camera         # + веб-камера (видео-кружки)
cargo build --release --features voice,camera   # + микрофон и звук (нужен alsa-lib)
./target/release/cybercord --name ВашеИмя
```

## Зависимости системы

| Система | Для звука (`voice`) | Для камеры (`camera`) |
|---|---|---|
| Arch / Artix | `sudo pacman -S alsa-lib` | ничего (V4L2 через ioctl), пользователь в группе `video` |
| Debian / Ubuntu | `sudo apt install libasound2-dev pkg-config` | то же |
| Fedora | `sudo dnf install alsa-lib-devel` | то же |
| Windows | ничего (WASAPI) | ничего (Media Foundation) |

Окно рисуется через eframe/egui (OpenGL), отдельный GTK не нужен.

## Параметры запуска

| Параметр | Значение |
|---|---|
| `--name ИМЯ` | отображаемое имя |
| `--headless` | консольный режим без окна (`/help` — команды) |
| `--bootstrap АДРЕС` | дозвониться до известной ноды при старте |
| `--no-mdns` | не искать в локальной сети |
| `--tcp-port N --quic-port N` | порты (по умолчанию 47100) |
| `--identity ФАЙЛ` | другой файл ключа |

Переменные окружения: `CYBERCORD_DATA_DIR` (папка данных, по умолчанию
`~/.local/share/cybercord`), `CYBERCORD_NO_GLOBAL=1` (без DHT BitTorrent),
`CYBERCORD_TEST_CAMERA=1` (тестовая камера), `CYBERCORD_ALLOW_LOOPBACK=1` (для тестов на одной машине).

## Иконки и иллюстрации

SVG генерируются скриптами, PNG — через `rsvg-convert`:

```bash
python3 tools/make_icons.py    # плитки серверов и символьные иконки
python3 tools/make_guide.py    # пошаговые иллюстрации docs/img/guide-*.svg
```
