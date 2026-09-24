"""Translation wrapper using deep-translator (free Google Translate, no API key)."""
import logging
from deep_translator import GoogleTranslator

logger = logging.getLogger("translator.engine")

class TranslateEngine:
    def __init__(self, source="en", target="ru"):
        self.source = source
        self.target = target
        # Pre-create translator to reuse connection
        try:
            self.translator = GoogleTranslator(source=source, target=target)
            logger.info(f"Translator initialized: {source} -> {target}")
        except Exception as exc:
            logger.error(f"Translator init failed: {exc}")
            self.translator = None

    def translate_text(self, text: str) -> str:
        if not text or not self.translator:
            return text
        try:
            result = self.translator.translate(text)
            return result
        except Exception as exc:
            logger.warning(f"Translation failed for '{text[:40]}...': {exc}")
            # Fallback: return original to avoid crash
            return text
