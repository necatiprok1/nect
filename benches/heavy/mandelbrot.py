# Mandelbrot set at 1500x1500 with an iteration cap of 200 (hundreds of millions
# of complex multiplications, data dependent).
# AI-relevant: procedural content generation and the escape-time pattern behind
# generative image models; also a stress test for floating-point fidelity.
def mandelbrot(cx, cy, max_iter):
    x = 0.0
    y = 0.0
    i = 0
    while i < max_iter:
        xx = x * x
        yy = y * y
        if xx + yy > 4.0:
            return i
        xy = x * y
        y = 2.0 * xy + cy
        x = xx - yy + cx
        i = i + 1
    return max_iter

width = 1500
height = 1500
max_iter = 200
bx = -2.0
sx = 0.0016666666666666668
by = -1.25
sy = 0.0016666666666666668
total = 0
for ix in range(width):
    for iy in range(height):
        total = total + mandelbrot(bx + ix * sx, by + iy * sy, max_iter)
print(f"mandelbrot total iterations: {total}")
