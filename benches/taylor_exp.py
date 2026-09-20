def taylor_exp(x, terms):
    s = 0.0
    term = 1.0
    for k in range(1, terms + 1):
        s += term
        term = term * x / k
    return s

result = taylor_exp(10.0, 100000)
print(f"taylor exp: {result}")