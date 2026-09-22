// ReLU (Rectified Linear Unit) activation
function relu_layer(n_neurons: number): number {
    let total: number = 0.0;
    for (let i: number = 0; i < n_neurons; i++) {
        const z: number = i * 0.01 - 2.5;
        if (z > 0.0) {
            total = total + z;
        }
    }
    return total;
}

const epochs: number = 10000;
let total: number = 0.0;
for (let epoch: number = 0; epoch < epochs; epoch++) {
    total = total + relu_layer(500);
}
console.log("relu total: " + total);