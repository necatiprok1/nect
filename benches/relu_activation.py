def relu_layer(n_neurons):
    total = 0.0
    for i in range(n_neurons):
        z = i * 0.01 - 2.5
        if z > 0.0:
            total += z
    return total

epochs = 10000
total = 0.0
for epoch in range(epochs):
    total += relu_layer(500)
print(f"relu total: {total}")