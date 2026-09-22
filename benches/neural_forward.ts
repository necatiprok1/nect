// Neural network forward pass
function dot_product(n_inputs: number, a_scale: number, b_scale: number): number {
    let sum: number = 0.0;
    for (let i: number = 0; i < n_inputs; i++) {
        const a: number = a_scale + i * 0.001;
        const b: number = b_scale + i * 0.002;
        sum = sum + a * b;
    }
    return sum;
}

const layers: number = 20;
const neurons: number = 50;
const n_inputs: number = 200;
let total: number = 0.0;
for (let layer: number = 0; layer < layers; layer++) {
    for (let neuron: number = 0; neuron < neurons; neuron++) {
        total = total + dot_product(n_inputs, layer * 0.1, neuron * 0.1);
    }
}
console.log("neural forward total: " + total);