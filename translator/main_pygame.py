#!/usr/bin/env python3
"""Pygame-based real-time loop (Linux prototype, no Qt/GL needed)."""
import os, sys, time, logging, threading
sys.path.insert(0, os.path.dirname(__file__))
logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(name)s: %(message)s")
logger = logging.getLogger("translator")

import pygame
from capture import ScreenCapture
from ocr_engine import OcrEngine
from translate_engine import TranslateEngine
from overlay_pygame import PygameOverlay

INTERVAL = 0.25  # 250ms

class App:
    def __init__(self):
        self.cap = ScreenCapture()
        self.ocr = OcrEngine()
        self.trans = TranslateEngine("en", "ru")
        self.ov = PygameOverlay(1920, 1080)
        self.running = True

    def tick(self):
        try:
            img = self.cap.capture_full()
            results = self.ocr.read_text(img)
            if not results:
                self.ov.set_items([
                    ("DynoTranslate: установите tesseract-ocr для авто-перевода", (200, 200, 1000, 60), 16, "#ffcc00")
                ])
            else:
                items = []
                for text, (x1, y1, x2, y2), conf in results:
                    if conf < 0.3:
                        continue
                    translated = self.trans.translate_text(text)
                    w = max(60, x2 - x1)
                    h = max(30, y2 - y1)
                    items.append((translated, (x1, y1, w, h), max(12, h//2), "#ffffff"))
                self.ov.set_items(items)
        except Exception as exc:
            logger.error(f"Tick error: {exc}")

    def run(self):
        clock = pygame.time.Clock()
        self.ov.set_items([("DynoTranslate работает — нажмите ESC", (200, 200, 1000, 60), 16, "#00ff88")])
        logger.info("DynoTranslate (pygame) started.")
        while self.running:
            for event in pygame.event.get():
                if event.type == pygame.QUIT:
                    self.running = False
                if event.type == pygame.KEYDOWN:
                    if event.key == pygame.K_ESCAPE:
                        self.running = False
            self.tick()
            self.ov.render()
            clock.tick(4)  # ~4 fps capture to save CPU; production uses 15-20 fps
        self.ov.quit()

if __name__ == "__main__":
    app = App()
    app.run()
