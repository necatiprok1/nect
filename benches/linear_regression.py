def train_regression(n_samples, iterations, lr):
    m, b = 0.0, 0.0
    for i in range(iterations):
        grad_m, grad_b = 0.0, 0.0
        for j in range(n_samples):
            x = j * 0.01
            y = 2.0 * x + 3.0
            pred = m * x + b
            error = pred - y
            grad_m += error * x
            grad_b += error
        m -= lr * grad_m / n_samples
        b -= lr * grad_b / n_samples
    return m + b

result = train_regression(1000, 2000, 0.01)
print(f"linear regression: {result}")