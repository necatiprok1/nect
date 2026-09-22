// Simulated matrix multiply: C[r,c] = sum_k A[r,k] * B[k,c]
function matmul_element(r: number, c: number, size: number): number {
    let sum: number = 0.0;
    for (let k: number = 0; k < size; k++) {
        const a: number = r * 0.0001 + k * 0.001;
        const b: number = k * 0.0001 + c * 0.001;
        sum = sum + a * b;
    }
    return sum;
}

const size: number = 60;
let total: number = 0.0;
for (let r: number = 0; r < size; r++) {
    for (let c: number = 0; c < size; c++) {
        total = total + matmul_element(r, c, size);
    }
}
console.log("matmul total: " + total);