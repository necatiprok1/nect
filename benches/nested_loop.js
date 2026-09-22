// Nested loop tensor contraction
let n = 60;
let total = 0.0;
for (let i = 0; i < n; i++) {
    for (let j = 0; j < n; j++) {
        for (let k = 0; k < n; k++) {
            total = total + (i + 1) * (j + 1) * (k + 1);
        }
    }
}
console.log("nested loop total: " + total);