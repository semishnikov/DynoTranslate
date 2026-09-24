#!/bin/bash
set -euo pipefail
echo "=== DynoTranslate Auto-Setup ==="
# Try to install tesseract and display libs (best effort)
sudo apt-get update -qq || true
sudo apt-get install -y -qq tesseract-ocr libtesseract-dev libxcb-randr0 libgl1-mesa-glx || echo "[WARN] apt install skipped (no network or no sudo)"
# Python venv
python3 -m venv venv || true
source venv/bin/activate
pip install --upgrade pip
pip install -r requirements.txt
# Download easyocr models if network allows (optional)
python3 -c "
import easyocr, os
try:
    reader = easyocr.Reader(['en','ru'], gpu=False, verbose=False)
    print('[OK] EasyOCR models available.')
except Exception as e:
    print('[INFO] EasyOCR models need download (run once with internet):', e)
" || true
echo "=== Setup complete ==="
echo "Run: python main_pygame.py  (or main.py for Qt version if libGL available)"
