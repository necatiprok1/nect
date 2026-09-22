// Squared Euclidean distance
function squared_distance(k) {
    let sum = 0.0;
    for (let i = 0; i < k; i++) {
        const a = i * 0.001 + 0.1;
        const b = i * 0.002 - 0.05;
        const diff = a - b;
        sum = sum + diff * diff;
    }
    return sum;
}

const k = 500;
let total = 0.0;
for (let run = 0; run < 1000; run++) {
    total = total + squared_distance(k);
}
console.log("euclidean distance total: " + total);