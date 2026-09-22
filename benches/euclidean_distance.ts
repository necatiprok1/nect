// Squared Euclidean distance
function squared_distance(k: number): number {
    let sum: number = 0.0;
    for (let i: number = 0; i < k; i++) {
        const a: number = i * 0.001 + 0.1;
        const b: number = i * 0.002 - 0.05;
        const diff: number = a - b;
        sum = sum + diff * diff;
    }
    return sum;
}

const k: number = 500;
let total: number = 0.0;
for (let run: number = 0; run < 1000; run++) {
    total = total + squared_distance(k);
}
console.log("euclidean distance total: " + total);