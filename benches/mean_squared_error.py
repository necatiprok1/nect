def squared_error_sum(n):
    s = 0.0
    for i in range(n):
        predicted = i * 0.001 + 0.5
        actual = i * 0.001 - 0.3
        diff = predicted - actual
        s += diff * diff
    return s

result = squared_error_sum(500000)
print(f"mse sum: {result}")