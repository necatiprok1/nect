// MSE sum (sum of squared errors)
function squared_error_sum(n: number): number {
    let sum: number = 0.0;
    for (let i: number = 0; i < n; i++) {
        const predicted: number = i * 0.001 + 0.5;
        const actual: number = i * 0.001 - 0.3;
        const diff: number = predicted - actual;
        sum = sum + diff * diff;
    }
    return sum;
}

const result: number = squared_error_sum(500000);
console.log("mse sum: " + result);