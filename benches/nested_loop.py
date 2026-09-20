n = 60
total = 0.0
for i in range(n):
    for j in range(n):
        for k in range(n):
            total += (i + 1) * (j + 1) * (k + 1)
print(f"nested loop total: {total}")