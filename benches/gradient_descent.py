def gradient_descent(x0, lr, iterations):
    x = x0
    while iterations > 0:
        grad = 2.0 * x + 5.0
        x = x - lr * grad
        iterations -= 1
    return x

result = gradient_descent(10.0, 0.01, 200000)
print(f"gradient_descent result: {result}")