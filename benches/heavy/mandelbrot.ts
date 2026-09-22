// Mandelbrot set at 1500x1500
function mandelbrot(cx: number, cy: number, max_iter: number): number {
    let x: number = 0.0;
    let y: number = 0.0;
    let i: number = 0;
    let xx: number = 0.0;
    let yy: number = 0.0;
    let xy: number = 0.0;
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

const width: number = 1500;
const height: number = 1500;
const max_iter: number = 200;
let total: number = 0;
const bx: number = -2.0;
const sx: number = 0.0016666666666666668;
const by: number = -1.25;
const sy: number = 0.0016666666666666668;
for (let ix: number = 0; ix < width; ix++) {
    for (let iy: number = 0; iy < height; iy++) {
        total = total + mandelbrot(bx + ix * sx, by + iy * sy, max_iter);
    }
}
console.log("mandelbrot total iterations: " + total);