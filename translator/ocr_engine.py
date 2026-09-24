"""OCR engine wrapper: tries pytesseract, then easyocr, then falls back."""
import os
import logging
from PIL import Image

logger = logging.getLogger("translator.ocr")

class OcrEngine:
    def __init__(self):
        self.use_pytesseract = False
        self.use_easyocr = False
        self._try_init()

    def _try_init(self):
        # Try pytesseract first (fastest, best for UI text)
        try:
            import pytesseract
            # Check binary by attempting a simple call
            result = pytesseract.image_to_string(Image.new("RGB", (10, 10), "white"))
            # If no exception, binary is present
            self.use_pytesseract = True
            logger.info("OCR: using pytesseract")
            return
        except Exception as e:
            logger.info(f"OCR: pytesseract unavailable ({e})")

        # Try easyocr (needs torch + downloaded models, best quality)
        try:
            import easyocr
            self.reader = easyocr.Reader(["en", "ru"], gpu=False, verbose=False)
            self.use_easyocr = True
            logger.info("OCR: using easyocr")
            return
        except Exception as e:
            logger.info(f"OCR: easyocr unavailable ({e})")

        logger.warning("OCR: no engine available; install tesseract-ocr (apt) or download easyocr models.")

    def read_text(self, image: Image.Image) -> list:
        """Return list of (text, bbox) where bbox is (x1, y1, x2, y2)."""
        results = []
        if self.use_pytesseract:
            try:
                import pytesseract
                # Use image_to_data for bbox info
                data = pytesseract.image_to_data(image, output_type=pytesseract.Output.DICT, lang="eng")
                n_boxes = len(data["text"])
                for i in range(n_boxes):
                    text = data["text"][i].strip()
                    if not text or text == "-1":
                        continue
                    conf = int(data["conf"][i])
                    if conf < 20:
                        continue
                    x, y, w, h = data["left"][i], data["top"][i], data["width"][i], data["height"][i]
                    results.append((text, (x, y, x + w, y + h), conf / 100.0))
            except Exception as exc:
                logger.error(f"pytesseract read error: {exc}")
        elif self.use_easyocr:
            try:
                # easyocr expects numpy array
                import numpy as np
                arr = np.array(image)
                # easyocr readtext returns ((bbox), text, conf)
                easy_results = self.reader.readtext(arr, detail=1, paragraph=False)
                for (bbox, text, conf) in easy_results:
                    # bbox is [[x1,y1],[x2,y2],[x3,y3],[x4,y4]]
                    xs = [p[0] for p in bbox]
                    ys = [p[1] for p in bbox]
                    x1, y1 = min(xs), min(ys)
                    x2, y2 = max(xs), max(ys)
                    results.append((text, (x1, y1, x2, y2), float(conf)))
            except Exception as exc:
                logger.error(f"easyocr read error: {exc}")
        else:
            # Fallback: try to extract from a synthetic region or return empty
            # For demo/real-time, we can return predefined test data if user selects region
            pass
        return results
