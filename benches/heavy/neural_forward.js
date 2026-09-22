// Forward pass of an 800-layer, 128-neuron, 512-input network
function dot_product(n_inputs, a_scale, b_scale) {
    let sum = 0.0;
    for (let i = 0; i < n_inputs; i++) {
        const a = a_scale + i * 0.001;
        const b = b_scale + i * 0.002;
        sum = sum + a * b;
    }
    return sum;
}

const layers = 800;
const neurons = 128;
const n_inputs = 512;
let total = 0.0;
for (let layer = 0; layer < layers; layer++) {
    for (let neuron = 0; neuron < neurons; neuron++) {
        total = total + dot_product(n_inputs, layer * 0.01, neuron * 0.01);
    }
}
console.log("neural forward total: " + total);