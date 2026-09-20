# Dot product: 8192-dimensional vectors, 16384 of them (134M multiply-adds).
# The inner loop is unrolled four ways with four independent accumulators, so
# the hardware can overlap the four multiply-add chains instead of stalling on
# one dependency chain. This is the shape native code is good at, and the same
# unrolling buys the interpreter almost nothing.
# AI-relevant: the dot product is the innermost operation of every dense layer,
# attention head (QK^T), and convolution kernel.
def dot4(n, scale):
    s0 = 0.0
    s1 = 0.0
    s2 = 0.0
    s3 = 0.0
    i = 0
    while i < n:
        a = i * 0.001 + scale
        b = i * 0.002 - scale
        s0 = s0 + a * b
        a = (i + 1) * 0.001 + scale
        b = (i + 1) * 0.002 - scale
        s1 = s1 + a * b
        a = (i + 2) * 0.001 + scale
        b = (i + 2) * 0.002 - scale
        s2 = s2 + a * b
        a = (i + 3) * 0.001 + scale
        b = (i + 3) * 0.002 - scale
        s3 = s3 + a * b
        i = i + 4
    return s0 + s1 + s2 + s3

n = 8192
reps = 16384
total = 0.0
r = 0
while r < reps:
    total = total + dot4(n, r * 0.0001)
    r = r + 1
print(f"dot product total: {total}")
