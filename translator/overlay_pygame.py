"""Pygame overlay for Linux prototype (works where Qt/GL missing)."""
import pygame, os, sys
pygame.init()

class PygameOverlay:
    def __init__(self, w=1920, h=1080):
        self.screen = pygame.display.set_mode((w, h), pygame.NOFRAME | pygame.SHOWN)
        pygame.display.set_caption("DynoTranslate Overlay")
        # Try to stay on top (WM hints) using wmctrl if available
        self._try_stay_on_top()
        self.font = pygame.font.SysFont("dejavusans", 18, bold=True)
        self.items = []

    def _try_stay_on_top(self):
        try:
            import subprocess, time
            # Find our window id and set always on top via wmctrl
            # Simplified: call wmctrl after a brief delay
            # Not reliable, but best effort
            pass
        except Exception:
            pass

    def set_items(self, items):
        """items: list of (text, bbox, font_size, color_hex)"""
        self.items = items

    def render(self):
        # Fill with transparent-ish black (simulate overlay)
        self.screen.fill((0, 0, 0))
        # Actually we want transparency; with pygame alone it's hard.
        # Instead draw dark semi-transparent rectangles and text.
        overlay = self.screen.copy()
        # We will draw directly
        for text, (x, y, w, h), size, hex_color in self.items:
            # Background pill
            rect = pygame.Rect(x, y, w, h)
            pygame.draw.rect(self.screen, (0, 0, 0), rect)
            pygame.draw.rect(self.screen, (255, 255, 255), rect, 1)
            # Text
            font = pygame.font.SysFont("dejavusans", size)
            color = tuple(int(hex_color[i:i+2], 16) for i in (1, 3, 5)) if hex_color.startswith("#") else (255, 255, 255)
            # Simple word wrap by splitting lines by length
            words = text.split(" ")
            lines = []
            current = ""
            for word in words:
                test = current + " " + word if current else word
                if font.size(test)[0] < w - 10:
                    current = test
                else:
                    lines.append(current)
                    current = word
            lines.append(current)
            y_line = y + 4
            for line in lines:
                surf = font.render(line, True, color)
                self.screen.blit(surf, (x + 4, y_line))
                y_line += font.get_height()
        pygame.display.flip()

    def quit(self):
        pygame.display.quit()
