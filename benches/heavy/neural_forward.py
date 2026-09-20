# Forward pass of an 800-layer, 128-neuron, 512-input network
# (52M weighted sums). Without arrays each input is derived from its index.
# AI-relevant: inference cost in transformer and MLP stacks is dominated by
# exactly this loop.
def dot_product(n_inputs, a_scale, b_scale):
    total = 0.0
    i = 0
    while i < n_inputs:
        a = a_scale + i * 0.001
        b = b_scale + i * 0.002
        total = total + a * b
        i = i + 1
    return total

layers = 800
neurons = 128
n_inputs = 512
total = 0.0
layer = 0
while layer < layers:
    neuron = 0
    while neuron < neurons:
        total = total + dot_product(n_inputs, layer * 0.01, neuron * 0.01)
        neuron = neuron + 1
    layer = layer + 1
print(f"neural forward total: {total}")
