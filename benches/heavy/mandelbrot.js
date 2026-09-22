// Mandelbrot set at 1500x1500
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

const width = 1500;
const height = 1500;
const max_iter = 200;
let total = 0;
const bx = -2.0;
const sx = 0.0016666666666666668;
const by = -1.25;
const sy = 0.0016666666666666668;
for (let ix = 0; ix < width; ix++) {
    for (let iy = 0; iy < height; iy++) {
        total = total + mandelbrot(bx + ix * sx, by + iy * sy, max_iter);
    }
}
console.log("mandelbrot total iterations: " + total);