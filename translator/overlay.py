"""Transparent overlay window using PyQt5."""
import sys
from PyQt5 import QtWidgets, QtCore, QtGui

class OverlayWindow(QtWidgets.QWidget):
    def __init__(self):
        super().__init__()
        self.setWindowFlags(
            QtCore.Qt.FramelessWindowHint |
            QtCore.Qt.WindowStaysOnTopHint |
            QtCore.Qt.Tool |
            QtCore.Qt.WindowTransparentForInput |
            QtCore.Qt.WindowDoesNotAcceptFocus
        )
        self.setAttribute(QtCore.Qt.WA_TranslucentBackground)
        self.setGeometry(0, 0, 1920, 1080)
        self.text_items = []  # list of (text, x, y, w, h, font_size)
        self.setStyleSheet("background: transparent;")

    def set_items(self, items):
        """items: list of (text, bbox, font_size, color_hex)"""
        self.text_items = items
        self.update()

    def paintEvent(self, event):
        painter = QtGui.QPainter(self)
        painter.setRenderHint(QtGui.QPainter.Antialiasing)
        for item in self.text_items:
            if len(item) >= 4:
                text = item[0]
                bbox = item[1]
                font_size = item[2] if len(item) > 2 else 16
                color_hex = item[3] if len(item) > 3 else "#ffffff"
                x, y, w, h = bbox
                # Draw background rect (slightly darker for readability)
                painter.fillRect(x, y, w, h, QtGui.QColor(0, 0, 0, 120))
                # Draw text
                painter.setPen(QtGui.QColor(color_hex))
                font = QtGui.QFont("DejaVu Sans", font_size)
                font.setBold(True)
                painter.setFont(font)
                # Word wrap inside bbox
                rect = QtCore.QRectF(x + 2, y + 2, w - 4, h - 4)
                painter.drawText(rect, QtCore.Qt.TextWordWrap, text)
        painter.end()

# Singleton instance
_overlay = None

def get_overlay():
    global _overlay
    if _overlay is None:
        _overlay = OverlayWindow()
        _overlay.show()
    return _overlay
