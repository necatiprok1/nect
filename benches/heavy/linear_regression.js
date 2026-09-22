// Batch gradient descent on y = 2x + 3
function train(n_samples, iterations, lr) {
    let m = 0.0;
    let b = 0.0;
    for (let i = 0; i < iterations; i++) {
        let grad_m = 0.0;
        let grad_b = 0.0;
        for (let j = 0; j < n_samples; j++) {
            const x = j * 0.01;
            const y = 2.0 * x + 3.0;
            const pred = m * x + b;
            const error = pred - y;
            grad_m = grad_m + error * x;
            grad_b = grad_b + error;
        }
        m = m - lr * grad_m * 0.0005;
        b = b - lr * grad_b * 0.0005;
    }
    return m + b;
}

const result = train(2000, 8000, 0.01);
console.log("linear regression: " + result);