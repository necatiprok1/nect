# Tensor contraction over three indices: 384^3 = 56.6M terms.
# AI-relevant: this is the loop shape behind einsum, batched matmul, and the
# memory-bound reductions inside attention and normalization layers.
n = 384
total = 0.0
for i in range(n):
    for j in range(n):
        for k in range(n):
            total = total + (i + 1) * (j + 1) * (k + 1)
print(f"tensor contraction total: {total}")
