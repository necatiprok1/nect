// Neural network forward pass
function dot_product(n_inputs, a_scale, b_scale) {
    let sum = 0.0;
    for (let i = 0; i < n_inputs; i++) {
        const a = a_scale + i * 0.001;
        const b = b_scale + i * 0.002;
        sum = sum + a * b;
    }
    return sum;
}

const layers = 20;
const neurons = 50;
const n_inputs = 200;
let total = 0.0;
for (let layer = 0; layer < layers; layer++) {
    for (let neuron = 0; neuron < neurons; neuron++) {
        total = total + dot_product(n_inputs, layer * 0.1, neuron * 0.1);
    }
}
console.log("neural forward total: " + total);