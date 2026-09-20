def integrate(n):
    s = 0.0
    h = 0.00001
    for i in range(n):
        x = (i + 0.5) * h
        s += x * x
    return s

result = integrate(1000000)
print(f"numerical integration: {result}")