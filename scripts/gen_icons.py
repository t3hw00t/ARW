#!/usr/bin/env python3
from PIL import Image, ImageDraw, ImageFilter

# Project aesthetic colors (docs/css/tokens.css)
COPPER = (0xB8, 0x73, 0x33)
COPPER_D = (0x91, 0x5A, 0x29)
TEAL   = (0x1B, 0xB3, 0xA3)
WHITE  = (255, 255, 255)
BLACK  = (8, 8, 8)

def gradient(size, c0, c1, diagonal=True):
    w, h = size
    img = Image.new('RGB', size, c0)
    pix = img.load()
    for y in range(h):
        for x in range(w):
            t = (x + y) / (w + h) if diagonal else (y / h)
            r = int(c0[0]*(1-t) + c1[0]*t)
            g = int(c0[1]*(1-t) + c1[1]*t)
            b = int(c0[2]*(1-t) + c1[2]*t)
            pix[x, y] = (r, g, b)
    return img

def draw_mark(size):
    # Minimal mark: stylized A with orbit arc
    w, h = size
    img = gradient(size, COPPER_D, COPPER)
    draw = ImageDraw.Draw(img)
    # Orbit arc
    bbox = [int(w*0.10), int(h*0.10), int(w*0.90), int(h*0.90)]
    draw.arc(bbox, start=210, end=330, width=max(2, w//32), fill=TEAL)
    # A shape (triangle + crossbar)
    apex = (w*0.50, h*0.22)
    left = (w*0.28, h*0.78)
    right= (w*0.72, h*0.78)
    draw.polygon([left, apex, right], fill=None, outline=WHITE)
    # inner stroke for thickness
    draw.line([left, apex], fill=WHITE, width=max(2, w//28))
    draw.line([apex, right], fill=WHITE, width=max(2, w//28))
    # crossbar
    draw.line([(w*0.38, h*0.56), (w*0.62, h*0.56)], fill=WHITE, width=max(2, w//28))
    # subtle vignette
    vign = Image.new('L', size, 0)
    dv = ImageDraw.Draw(vign)
    dv.ellipse([int(w*0.00), int(h*0.00), int(w*1.00), int(h*1.00)], fill=180)
    vign = vign.filter(ImageFilter.GaussianBlur(radius=max(2, w//24)))
    img = Image.composite(img, Image.new('RGB', size, BLACK), vign)
    return img

def main():
    outdir = 'apps/arw-launcher/src-tauri/icons'
    sizes = [16, 32, 48, 64, 128, 256, 512, 1024]
    for s in sizes:
        img = draw_mark((s, s))
        img.save(f'{outdir}/{s}x{s}.png')
    # Build a Windows .ico from multiple sizes
    imgs = [Image.open(f'{outdir}/{s}x{s}.png') for s in [16, 32, 48, 64, 128, 256]]
    imgs[0].save(f'{outdir}/icon.ico', sizes=[im.size for im in imgs], format='ICO')

if __name__ == '__main__':
    main()
