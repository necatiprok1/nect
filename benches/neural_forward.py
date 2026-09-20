def dot_product(n_inputs, a_scale, b_scale):
    s = 0.0
    for i in range(n_inputs):
        a = a_scale + i * 0.001
        b = b_scale + i * 0.002
        s += a * b
    return s

layers, neurons, n_inputs = 20, 50, 200
total = 0.0
for layer in range(layers):
    for neuron in range(neurons):
        total += dot_product(n_inputs, layer * 0.1, neuron * 0.1)
print(f"neural forward total: {total}")