// MSE sum (sum of squared errors)
function squared_error_sum(n) {
    let sum = 0.0;
    for (let i = 0; i < n; i++) {
        const predicted = i * 0.001 + 0.5;
        const actual = i * 0.001 - 0.3;
        const diff = predicted - actual;
        sum = sum + diff * diff;
    }
    return sum;
}

const result = squared_error_sum(500000);
console.log("mse sum: " + result);