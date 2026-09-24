#!/usr/bin/env python3
"""Qt-based main loop with timer for real-time updates."""
import os, sys, time, logging
sys.path.insert(0, os.path.dirname(__file__))

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(name)s: %(message)s")
logger = logging.getLogger("translator")

from PyQt5 import QtWidgets, QtCore
from capture import ScreenCapture
from ocr_engine import OcrEngine
from translate_engine import TranslateEngine
from overlay import OverlayWindow

SOURCE_LANG = "en"
TARGET_LANG = "ru"
INTERVAL_MS = 250

class TranslaterApp(QtWidgets.QApplication):
    def __init__(self, argv):
        super().__init__(argv)
        self.cap = ScreenCapture()
        self.ocr = OcrEngine()
        self.trans = TranslateEngine(SOURCE_LANG, TARGET_LANG)
        self.overlay = OverlayWindow()
        self.overlay.show()
        # Timer for capture/translate/render
        self.timer = QtCore.QTimer()
        self.timer.timeout.connect(self.tick)
        self.timer.start(INTERVAL_MS)

    def tick(self):
        try:
            img = self.cap.capture_full()
            results = self.ocr.read_text(img)
            if not results:
                self.overlay.set_items([
                    ("DynoTranslate: установите tesseract-ocr для авто-перевода", (200, 200, 1000, 60), 16, "#ffcc00")
                ])
                return
            items = []
            for text, (x1, y1, x2, y2), conf in results:
                if conf < 0.3:
                    continue
                translated = self.trans.translate_text(text)
                w = max(60, x2 - x1)
                h = max(30, y2 - y1)
                # Simplify: draw at original position; production uses window-relative mapping
                items.append((translated, (x1, y1, w, h), max(12, h // 2), "#ffffff"))
            self.overlay.set_items(items)
        except Exception as exc:
            logger.error(f"Tick error: {exc}")

if __name__ == "__main__":
    app = TranslaterApp(sys.argv)
    sys.exit(app.exec_())
