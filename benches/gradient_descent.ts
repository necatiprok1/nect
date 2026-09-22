// Gradient descent: optimize f(x) = x^2 + 5*x + 6
function gradient_descent(x0: number, lr: number, iterations: number): number {
    let x: number = x0;
    let grad: number = 0.0;
    while (iterations > 0) {
        grad = 2.0 * x + 5.0;
        x = x - lr * grad;
        iterations = iterations - 1;
    }
    return x;
}

const result: number = gradient_descent(10.0, 0.01, 200000);
console.log("gradient_descent result: " + result);