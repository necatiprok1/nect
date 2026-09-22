// Tensor contraction over three indices
const n: number = 384;
let total: number = 0.0;
for (let i: number = 0; i < n; i++) {
    for (let j: number = 0; j < n; j++) {
        for (let k: number = 0; k < n; k++) {
            total = total + (i + 1) * (j + 1) * (k + 1);
        }
    }
}
console.log("tensor contraction total: " + total);