// Geometric series: discounted return sum in reinforcement learning.
function geometric_sum(gamma: number, n: number): number {
    let sum: number = 0.0;
    let term: number = 1.0;
    while (n > 0) {
        sum = sum + term;
        term = term * gamma;
        n = n - 1;
    }
    return sum;
}

const result: number = geometric_sum(0.9999, 2000000);
console.log("geometric sum: " + result);