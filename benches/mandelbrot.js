// Mandelbrot set: escape-time fractal computation
function mandelbrot(cx, cy, max_iter) {
    let x = 0.0;
    let y = 0.0;
    let i = 0;
    let xx = 0.0;
    let yy = 0.0;
    let xy = 0.0;
    while (i < max_iter) {
        xx = x * x;
        yy = y * y;
        if (xx + yy > 4.0) {
            return i;
        }
        xy = x * y;
        y = 2.0 * xy + cy;
        x = xx - yy + cx;
        i = i + 1;
    }
    return max_iter;
}

const width = 80;
const height = 80;
const max_iter = 100;
let total = 0;
const bx = -2.0;
const by = -1.5;
const sx = 0.0375;
const sy = 0.0375;
for (let ix = 0; ix < width; ix++) {
    for (let iy = 0; iy < height; iy++) {
        const cx = bx + ix * sx;
        const cy = by + iy * sy;
        total = total + mandelbrot(cx, cy, max_iter);
    }
}
console.log("mandelbrot total iterations: " + total);