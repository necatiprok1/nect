// Simulated matrix multiply: C[r,c] = sum_k A[r,k] * B[k,c]
function matmul_element(r, c, size) {
    let sum = 0.0;
    for (let k = 0; k < size; k++) {
        const a = r * 0.0001 + k * 0.001;
        const b = k * 0.0001 + c * 0.001;
        sum = sum + a * b;
    }
    return sum;
}

const size = 60;
let total = 0.0;
for (let r = 0; r < size; r++) {
    for (let c = 0; c < size; c++) {
        total = total + matmul_element(r, c, size);
    }
}
console.log("matmul total: " + total);