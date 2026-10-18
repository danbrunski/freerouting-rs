"""How tall the route dialog's tab pages are (plugin.Dialog).

The dialog holds its options on tab pages that scroll, and a notebook is as
tall as its tallest page. Fitted, the dialog can come out taller than the
screen, which hid the buttons at the bottom; then the pages are cut down and
scroll.
"""

# Of the display's work area (without the taskbar), the share the dialog may take.
SCREEN_SHARE = 0.9
# No page is cut below this many pixels.
MIN_PAGE_HEIGHT = 160


def max_dialog_height(work_area_height):
    return int(work_area_height * SCREEN_SHARE)


def cut_to_screen(heights, dialog_height, max_height, floor=MIN_PAGE_HEIGHT):
    """The page heights that bring a dialog fitted at `dialog_height` with
    pages of `heights` down to `max_height`, or None when it already fits.
    The tallest page shrinks by the excess and the others only down to it."""
    excess = dialog_height - max_height
    if excess <= 0 or not heights:
        return None
    room = max(max(heights) - excess, floor)
    return [min(height, room) for height in heights]
