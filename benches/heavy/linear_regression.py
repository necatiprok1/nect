# Batch gradient descent on y = 2x + 3: 8000 epochs over 2000 samples
# (16M inner steps). Division is spelled as multiplication by 0.0005 so the
# inner loop stays a pure multiply-add.
# AI-relevant: training a linear model is the smallest end-to-end example of
# the optimization loop that trains every neural network.
def train(n_samples, iterations, lr):
    m = 0.0
    b = 0.0
    i = 0
    while i < iterations:
        grad_m = 0.0
        grad_b = 0.0
        j = 0
        while j < n_samples:
            x = j * 0.01
            y = 2.0 * x + 3.0
            pred = m * x + b
            error = pred - y
            grad_m = grad_m + error * x
            grad_b = grad_b + error
            j = j + 1
        m = m - lr * grad_m * 0.0005
        b = b - lr * grad_b * 0.0005
        i = i + 1
    return m + b

print(f"linear regression: {train(2000, 8000, 0.01)}")
