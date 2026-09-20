def squared_distance(k):
    s = 0.0
    for i in range(k):
        a = i * 0.001 + 0.1
        b = i * 0.002 - 0.05
        diff = a - b
        s += diff * diff
    return s

k = 500
total = 0.0
for run in range(1000):
    total += squared_distance(k)
print(f"euclidean distance total: {total}")