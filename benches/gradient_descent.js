// Gradient descent: optimize f(x) = x^2 + 5*x + 6
function gradient_descent(x0, lr, iterations) {
    let x = x0;
    let grad = 0.0;
    while (iterations > 0) {
        grad = 2.0 * x + 5.0;
        x = x - lr * grad;
        iterations = iterations - 1;
    }
    return x;
}

const result = gradient_descent(10.0, 0.01, 200000);
console.log("gradient_descent result: " + result);