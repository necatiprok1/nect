// Forward pass of an 800-layer, 128-neuron, 512-input network
function dot_product(n_inputs: number, a_scale: number, b_scale: number): number {
    let sum: number = 0.0;
    for (let i: number = 0; i < n_inputs; i++) {
        const a: number = a_scale + i * 0.001;
        const b: number = b_scale + i * 0.002;
        sum = sum + a * b;
    }
    return sum;
}

const layers: number = 800;
const neurons: number = 128;
const n_inputs: number = 512;
let total: number = 0.0;
for (let layer: number = 0; layer < layers; layer++) {
    for (let neuron: number = 0; neuron < neurons; neuron++) {
        total = total + dot_product(n_inputs, layer * 0.01, neuron * 0.01);
    }
}
console.log("neural forward total: " + total);