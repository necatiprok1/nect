// Dot product: 8192-dimensional vectors
function dot4(n: number, scale: number): number {
    let s0: number = 0.0;
    let s1: number = 0.0;
    let s2: number = 0.0;
    let s3: number = 0.0;
    for (let i: number = 0; i < n; i += 4) {
        let a: number = i * 0.001 + scale;
        let b: number = i * 0.002 - scale;
        s0 = s0 + a * b;
        a = (i + 1) * 0.001 + scale;
        b = (i + 1) * 0.002 - scale;
        s1 = s1 + a * b;
        a = (i + 2) * 0.001 + scale;
        b = (i + 2) * 0.002 - scale;
        s2 = s2 + a * b;
        a = (i + 3) * 0.001 + scale;
        b = (i + 3) * 0.002 - scale;
        s3 = s3 + a * b;
    }
    return s0 + s1 + s2 + s3;
}

const n: number = 8192;
const reps: number = 16384;
let total: number = 0.0;
for (let r: number = 0; r < reps; r++) {
    total = total + dot4(n, r * 0.0001);
}
console.log("dot product total: " + total);