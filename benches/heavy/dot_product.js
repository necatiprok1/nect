// Dot product: 8192-dimensional vectors
function dot4(n, scale) {
    let s0 = 0.0;
    let s1 = 0.0;
    let s2 = 0.0;
    let s3 = 0.0;
    let i = 0;
    while (i < n) {
        let a = i * 0.001 + scale;
        let b = i * 0.002 - scale;
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
        i = i + 4;
    }
    return s0 + s1 + s2 + s3;
}

const n = 8192;
const reps = 16384;
let total = 0.0;
for (let r = 0; r < reps; r++) {
    total = total + dot4(n, r * 0.0001);
}
console.log("dot product total: " + total);