# Icons (R01.3)

`icon.svg` is the one source: an original drawing (two routed nets with 45-degree bends on a
dark green board), plain shapes only. It does not imitate the Java Freerouting logo (R00
question 3). Every other file here is generated from it:

```sh
python3 crates/freerouting-gui/icons/render.py
```

`render.py` needs ImageMagick's `convert` (draws the SVG once at 1024 px) and Pillow. It writes

- `icon-16.png` ... `icon-1024.png` (16, 32, 48, 64, 128, 256, 512, 1024),
- `icon.ico` (PNG entries at 16, 24, 32, 48, 64, 128, 256; Windows resource and MSI),
- `icon.icns` (PNG entries 16 to 1024; the macOS app bundle).

The output is deterministic, so a re-run leaves `git status` clean. K03.1's 64 px plugin icon
is `icon-64.png` (copy it to `kicad-plugin/resources/icon.png`).

The owner approves the artwork before the first public release (R00 question 3); until then it is a
placeholder, and replacing `icon.svg` plus a re-run of `render.py` is the whole change.
