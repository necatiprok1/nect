def factorial(n):
    result = 1
    i = 1
    while i <= n:
        result = result * i
        i = i + 1
    return result

n = 100
result = factorial(n)
print(f"factorial({n}) = {result}")
