def geometric_sum(gamma, n):
    s = 0.0
    term = 1.0
    while n > 0:
        s += term
        term *= gamma
        n -= 1
    return s

result = geometric_sum(0.9999, 2000000)
print(f"geometric sum: {result}")