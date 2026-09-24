"""Screen capture using mss."""
from PIL import Image
import mss
import mss.tools

class ScreenCapture:
    def __init__(self):
        self.sct = mss.mss()
        self.monitors = self.sct.monitors  # 0 = all monitors combined

    def capture_region(self, bbox: tuple) -> Image.Image:
        """bbox = (left, top, width, height)"""
        left, top, width, height = bbox
        # mss uses (left, top, right, bottom)
        region = {"left": left, "top": top, "width": width, "height": height}
        sct_img = self.sct.grab(region)
        return Image.frombytes("RGB", sct_img.size, sct_img.bgra, "raw", "BGRX")

    def capture_full(self) -> Image.Image:
        # Use first monitor (index 1) for focused capture, or 0 for all
        sct_img = self.sct.grab(self.sct.monitors[1])
        return Image.frombytes("RGB", sct_img.size, sct_img.bgra, "raw", "BGRX")
