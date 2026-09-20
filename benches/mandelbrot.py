def mandelbrot(cx, cy, max_iter):
    x, y = 0.0, 0.0
    i = 0
    while i < max_iter:
        xx, yy = x * x, y * y
        if xx + yy > 4.0:
            return i
        xy = x * y
        y = 2.0 * xy + cy
        x = xx - yy + cx
        i += 1
    return max_iter

width, height, max_iter = 80, 80, 100
total = 0
bx, by = -2.0, -1.5
sx, sy = 0.0375, 0.0375
for ix in range(width):
    for iy in range(height):
        cx = bx + ix * sx
        cy = by + iy * sy
        total += mandelbrot(cx, cy, max_iter)
print(f"mandelbrot total iterations: {total}")