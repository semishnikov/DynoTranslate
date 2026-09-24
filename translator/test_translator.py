#!/usr/bin/env python3
"""Unit tests for translator pipeline (no network required for mocks)."""
import unittest, sys, os
sys.path.insert(0, os.path.dirname(__file__))
from PIL import Image, ImageDraw, ImageFont

class TestTranslatorPipeline(unittest.TestCase):
    def test_translate_engine_fallback(self):
        from translate_engine import TranslateEngine
        t = TranslateEngine("en", "ru")
        # Network unavailable in sandbox; should return original gracefully
        result = t.translate_text("Hello")
        self.assertIn(result, ["Hello", "Привет"])

    def test_ocr_engine_init(self):
        from ocr_engine import OcrEngine
        ocr = OcrEngine()
        # Should initialize without crash; may report no binary
        self.assertTrue(hasattr(ocr, "use_pytesseract") or hasattr(ocr, "use_easyocr"))

    def test_capture_init(self):
        # Capture requires display; if unavailable it raises; test that class exists
        from capture import ScreenCapture
        self.assertTrue(callable(ScreenCapture))

    def test_overlay_items(self):
        from overlay_pygame import PygameOverlay
        # Just ensure class exists; window creation needs display
        self.assertTrue(hasattr(PygameOverlay, "set_items"))

    def test_synthetic_ocr_image(self):
        # Create an image with text, verify it can be saved
        img = Image.new("RGB", (400, 200), (30, 30, 30))
        draw = ImageDraw.Draw(img)
        try:
            font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 24)
        except Exception:
            font = ImageFont.load_default()
        draw.text((20, 80), "Start Game", fill=(255, 255, 255), font=font)
        img.save("test_ocr_image.png")
        self.assertTrue(os.path.exists("test_ocr_image.png"))

if __name__ == "__main__":
    unittest.main(verbosity=2)
