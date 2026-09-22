// ReLU (Rectified Linear Unit) activation
function relu_layer(n_neurons) {
    let total = 0.0;
    for (let i = 0; i < n_neurons; i++) {
        const z = i * 0.01 - 2.5;
        if (z > 0.0) {
            total = total + z;
        }
    }
    return total;
}

const epochs = 10000;
let total = 0.0;
for (let epoch = 0; epoch < epochs; epoch++) {
    total = total + relu_layer(500);
}
console.log("relu total: " + total);