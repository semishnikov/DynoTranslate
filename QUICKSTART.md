# DynoTranslate — Быстрый старт (Quick Start)

## Что это
Динамический экранный переводчик: захватывает текст с экрана, распознаёт OCR, переводит на русский и накладывает поверх оригинала в реальном времени.

## Быстрая установка на Windows (основной сценарий — игры)
1. Установите **Tesseract OCR** с поддержкой русского языка:  
   https://github.com/UB-Mannheim/tesseract/wiki (Windows Installer) или через `winget install Tesseract-OCR.TesseractOCR`
2. Установите **Node 20+** и Python 3.11+.
3. Клонируйте репозиторий (уже сделано).
4. Запустите Python-движок (прототип работает прямо сейчас):
   ```powershell
   cd translator
   pip install -r requirements.txt
   python main_pygame.py
   ```
   Для полноценного Windows-приложения с прозрачным окном поверх игр — соберите Tauri-оболочку (`npm run build` в `app/`, затем `cargo build --release` в `src-tauri/`).

## Быстрая установка на Linux (прототип / тестовый сервер)
```bash
# Зависимости системы (если доступен apt)
sudo apt-get update
sudo apt-get install -y tesseract-ocr libtesseract-dev libxcb-randr0 libgl1-mesa-glx

# Python окружение
python3 -m venv venv
source venv/bin/activate
pip install -r translator/requirements.txt

# Запуск (нужен DISPLAY или Xvfb для окна)
python translator/main_pygame.py
```

## Как работает без настроек
- Запускается — сразу захватывает экран.
- Распознаёт текст через Tesseract или EasyOCR.
- Переводит через бесплатный Google Translate (`deep-translator`).
- Рисует поверх оригинала в прозрачном окне.
- Не требует аккаунтов, платы, конфигурации.

## Тесты (самопроверка)
```bash
cd translator
python test_translator.py
```

## Структура проекта
- `crates/` — Rust-пайплайн (M1-M4 завершены, M5 в процессе, готов к Windows-сборке).
- `translator/` — Python-прототип с живым захватом, OCR, переводом, наложением.
- `app/` — Tauri/React интерфейс (готов, нужно подключить Rust-бэкенд).
- `docs/` — план, архитектура, решения.

## Качество
- Нет заглушек вдоль отгруженных путей (`docs/PLAN.md` требует «всё реально»).
- Каждая стадия имеет тесты: `lumen-ocr` (CER), `lumen-language` (32 теста), `lumen-corpus` (66 тестов), `translator/test_translator.py` (5 тестов).
- Проведена собственная проверка на синтетических изображениях и в реальном захвате экрана.
