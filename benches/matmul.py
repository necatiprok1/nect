def matmul_element(r, c, size):
    s = 0.0
    for k in range(size):
        a = r * 0.0001 + k * 0.001
        b = k * 0.0001 + c * 0.001
        s += a * b
    return s

size = 60
total = 0.0
for r in range(size):
    for c in range(size):
        total += matmul_element(r, c, size)
print(f"matmul total: {total}")