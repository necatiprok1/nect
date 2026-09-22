// Linear regression: batch gradient descent
function train_regression(n_samples: number, iterations: number, lr: number): number {
    let m: number = 0.0;
    let b: number = 0.0;
    for (let i: number = 0; i < iterations; i++) {
        let grad_m: number = 0.0;
        let grad_b: number = 0.0;
        for (let j: number = 0; j < n_samples; j++) {
            const x: number = j * 0.01;
            const y: number = 2.0 * x + 3.0;
            const pred: number = m * x + b;
            const error: number = pred - y;
            grad_m = grad_m + error * x;
            grad_b = grad_b + error;
        }
        // Average gradient (n_samples = 1000, so divide by 1000 = multiply by 0.001)
        m = m - lr * grad_m * 0.001;
        b = b - lr * grad_b * 0.001;
    }
    return m + b;
}

const result: number = train_regression(1000, 2000, 0.01);
console.log("linear regression: " + result);